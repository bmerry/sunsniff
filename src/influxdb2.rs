use async_trait::async_trait;
use futures::channel::mpsc::UnboundedReceiver;
use futures::stream::{self, StreamExt};
use influxdb2::models::DataPoint;
use influxdb2::Client;
use std::iter::zip;
use std::sync::Arc;

use super::receiver::{Receiver, Update};

pub struct Influxdb2Receiver {
    client: Client,
    bucket: String,
}

impl Influxdb2Receiver {
    pub fn new(client: Client, bucket: impl Into<String>) -> Influxdb2Receiver {
        return Influxdb2Receiver {
            client,
            bucket: bucket.into(),
        };
    }
}

#[async_trait]
impl Receiver for Influxdb2Receiver {
    async fn run<'a>(&mut self, mut receiver: UnboundedReceiver<Arc<Update<'a>>>) {
        while let Some(update) = receiver.next().await {
            let mut points = vec![];
            for (field, value) in zip(update.fields.iter(), update.values.iter()) {
                points.push(
                    DataPoint::builder("inverter")
                        .timestamp(update.timestamp)
                        .tag("serial", update.serial.as_str())
                        .tag("group", field.group)
                        .tag("name", field.name)
                        .tag("unit", field.unit)
                        .field("value", *value)
                        .build()
                        .unwrap(), // TODO: handle errors
                );
            }
            // TODO: handle error
            self.client
                .write(self.bucket.as_str(), stream::iter(points))
                .await
                .unwrap();
        }
    }
}
