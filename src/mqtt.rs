use async_trait::async_trait;
use futures::channel::mpsc::UnboundedReceiver;
use futures::stream::StreamExt;
use mqtt_async_client::client::{Client, Publish, QoS};
use phf::phf_map;
use serde::{self, Deserialize, Serialize};
use serde_json;
use std::iter::zip;
use std::sync::Arc;

use super::receiver::{Field, Receiver, Update};

struct ClassInfo<'a> {
    device_class: Option<&'a str>,
    state_class: &'a str,
}

impl<'a> ClassInfo<'a> {
    const fn new(device_class: &'a str, state_class: &'a str) -> Self {
        ClassInfo{device_class: Some(device_class), state_class}
    }

    const fn new_no_device(state_class: &'a str) -> Self {
        ClassInfo{device_class: None, state_class}
    }
}

// TODO: add a level of abstraction to Field. Should be able to index by enum, not str
static CLASSES: phf::Map<&'static str, ClassInfo<'static>> = phf_map! {
    "°C" => ClassInfo::new("temperature", "measurement"),
    "W" => ClassInfo::new("power", "measurement"),
    "A" => ClassInfo::new("current", "measurement"),
    "V" => ClassInfo::new("voltage", "measurement"),
    "kWh" => ClassInfo::new("energy", "total_increasing"),
    "%" => ClassInfo::new("battery", "measurement"),
    "Hz" => ClassInfo::new_no_device("measurement"),
};

#[derive(Serialize)]
struct Device<'a> {
    identifiers: (&'a str,),
}

#[derive(Serialize)]
struct Sensor<'a> {
    device: Device<'a>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    device_class: Option<&'a str>,
    expire_after: i32,
    name: &'a str,
    object_id: &'a str,
    state_class: &'a str,
    state_topic: &'a str,
    unique_id: &'a str,
    unit_of_measurement: &'a str,
}

pub struct MqttReceiver {
    client: Client,
}

fn state_topic<'a>(field: &Field<'a>, serial: &str) -> String {
    format!(
        "homeassistant/sensor/sunsniff_{}_{}/state",
        serial, field.id
    )
}

fn config_topic<'a>(field: &Field<'a>, serial: &str) -> String {
    format!(
        "homeassistant/sensor/sunsniff_{}_{}/config",
        serial, field.id
    )
}

impl MqttReceiver {
    pub fn new(config: &Config) -> mqtt_async_client::Result<Self> {
        let client = Client::builder()
            .set_url_string(&config.url)?
            .set_username(config.username.clone())
            .set_password(config.password.as_ref().map(|s| s.as_bytes().to_vec()))
            .build()?;
        Ok(MqttReceiver { client })
    }

    async fn register_field<'a>(
        &self,
        field: &Field<'a>,
        serial: &str,
    ) -> mqtt_async_client::Result<()> {
        let state_topic_str = state_topic(field, serial);
        let config_topic_str = config_topic(field, serial);
        let unique_id = format!("sunsniff_{}_{}", serial, field.id);
        let full_name = format!("{} {}", field.group, field.name);
        let class_info = CLASSES.get(field.unit).unwrap();  // TODO: deal better with errors
        let sensor = Sensor {
            device: Device { identifiers: (serial,) },
            device_class: class_info.device_class,
            expire_after: 600,
            name: &full_name,
            object_id: &unique_id,
            state_class: class_info.state_class,
            state_topic: state_topic_str.as_str(),
            unique_id: &unique_id,
            unit_of_measurement: field.unit,
        };
        // TODO: more graceful error handling on to_vec
        let mut msg = Publish::new(config_topic_str, serde_json::to_vec(&sensor).unwrap());
        let msg = msg.set_retain(true).set_qos(QoS::AtLeastOnce);
        self.client.publish(&msg).await?;
        Ok(())
    }
}

#[async_trait]
impl Receiver for MqttReceiver {
    async fn run<'a>(&mut self, mut receiver: UnboundedReceiver<Arc<Update<'a>>>) {
        self.client
            .connect()
            .await
            .unwrap_or_else(|e| eprintln!("Couldn't connect to MQTT broker: {}", e));
        while let Some(update) = receiver.next().await {
            for (field, value) in zip(update.fields.iter(), update.values.iter()) {
                // TODO: don't reregister on every update
                self.register_field(field, &update.serial)
                    .await
                    .unwrap_or_else(|e| eprintln!("Registration failed: {}", e));
                let payload = value.to_string().as_bytes().to_vec();
                let msg = Publish::new(state_topic(field, &update.serial), payload);
                self.client
                    .publish(&msg)
                    .await
                    .unwrap_or_else(|e| eprintln!("Sending update failed: {}", e));
            }
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub url: String,
    pub username: Option<String>,
    pub password: Option<String>,
}
