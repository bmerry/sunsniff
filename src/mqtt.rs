/* Copyright 2022 Bruce Merry
 *
 * This program is free software: you can redistribute it and/or modify it
 * under the terms of the GNU General Public License as published by the Free
 * Software Foundation, either version 3 of the License, or (at your option)
 * any later version.
 *
 * This program is distributed in the hope that it will be useful, but WITHOUT
 * ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or
 * FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for
 * more details.
 *
 * You should have received a copy of the GNU General Public License along
 * with this program. If not, see <https://www.gnu.org/licenses/>.
 */

use async_trait::async_trait;
use futures::channel::mpsc::UnboundedReceiver;
use futures::stream::StreamExt;
use log::warn;
use mqtt_async_client::client::{Client, Publish, QoS};
use serde::{self, Deserialize, Serialize};
use serde_json;
use std::collections::HashSet;
use std::iter::zip;
use std::sync::Arc;

use super::fields::{Field, FieldType};
use super::receiver::{Receiver, Update};

struct ClassInfo<'a> {
    device_class: Option<&'a str>,
    state_class: &'a str,
}

impl<'a> ClassInfo<'a> {
    const fn new(device_class: &'a str, state_class: &'a str) -> Self {
        ClassInfo {
            device_class: Some(device_class),
            state_class,
        }
    }

    const fn new_no_device(state_class: &'a str) -> Self {
        ClassInfo {
            device_class: None,
            state_class,
        }
    }
}

impl From<FieldType> for ClassInfo<'static> {
    fn from(ft: FieldType) -> Self {
        match ft {
            FieldType::Charge | FieldType::Unitless => ClassInfo::new_no_device("measurement"),
            FieldType::Current => ClassInfo::new("current", "measurement"),
            FieldType::Energy => ClassInfo::new("energy", "total_increasing"),
            FieldType::Frequency => ClassInfo::new_no_device("measurement"),
            FieldType::Power => ClassInfo::new("power", "measurement"),
            FieldType::StateOfCharge => ClassInfo::new("battery", "measurement"),
            FieldType::Temperature => ClassInfo::new("temperature", "measurement"),
            FieldType::Voltage => ClassInfo::new("voltage", "measurement"),
        }
    }
}

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

/// Field associated with a specific device
struct DeviceField<'a> {
    field: &'a Field<'a>,
    serial: &'a str,
    unique_id: String,
    state_topic: String,
    config_topic: String,
}

impl<'a> DeviceField<'a> {
    fn new(field: &'a Field<'a>, serial: &'a str) -> Self {
        let unique_id = format!("sunsniff_{}_{}", serial, field.id);
        let state_topic = format!("homeassistant/sensor/{unique_id}/state");
        let config_topic = format!("homeassistant/sensor/{unique_id}/config");
        Self {
            field,
            serial,
            unique_id,
            state_topic,
            config_topic,
        }
    }
}

pub struct MqttReceiver {
    client: Client,
    registered: HashSet<String>,
}

impl MqttReceiver {
    pub fn new(config: &Config) -> mqtt_async_client::Result<Self> {
        let client = Client::builder()
            .set_url_string(&config.url)?
            .set_username(config.username.clone())
            .set_password(config.password.as_ref().map(|s| s.as_bytes().to_vec()))
            .build()?;
        Ok(MqttReceiver {
            client,
            registered: HashSet::new(),
        })
    }

    async fn register_field<'a>(
        &mut self,
        field: &DeviceField<'a>,
    ) -> mqtt_async_client::Result<()> {
        if !self.registered.contains(&field.unique_id) {
            let full_name = format!("{} {}", field.field.group, field.field.name);
            let class_info: ClassInfo = field.field.field_type.into();
            let sensor = Sensor {
                device: Device {
                    identifiers: (field.serial,),
                },
                device_class: class_info.device_class,
                expire_after: 600,
                name: &full_name,
                object_id: &field.unique_id,
                state_class: class_info.state_class,
                state_topic: &field.state_topic,
                unique_id: &field.unique_id,
                unit_of_measurement: field.field.unit,
            };
            // TODO: more graceful error handling on to_vec
            let mut msg = Publish::new(
                field.config_topic.to_owned(),
                serde_json::to_vec(&sensor).unwrap(),
            );
            let msg = msg.set_retain(true).set_qos(QoS::AtLeastOnce);
            self.client.publish(msg).await?;
            self.registered.insert(field.unique_id.to_owned());
        }
        Ok(())
    }
}

#[async_trait]
impl Receiver for MqttReceiver {
    async fn run<'a>(&mut self, mut receiver: UnboundedReceiver<Arc<Update<'a>>>) {
        self.client
            .connect()
            .await
            .unwrap_or_else(|e| warn!("Couldn't connect to MQTT broker (will keep trying): {}", e));
        while let Some(update) = receiver.next().await {
            for (field, value) in zip(update.fields.iter(), update.values.iter()) {
                let device_field = DeviceField::new(field, &update.serial);
                self.register_field(&device_field)
                    .await
                    .unwrap_or_else(|e| warn!("Registering {} failed: {}", field.id, e));
                let payload = value.to_string().as_bytes().to_vec();
                let msg = Publish::new(device_field.state_topic, payload);
                self.client
                    .publish(&msg)
                    .await
                    .unwrap_or_else(|e| warn!("Sending update for {} failed: {}", field.id, e));
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
