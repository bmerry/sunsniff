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

use async_std::task;
use async_trait::async_trait;
use futures::channel::mpsc::UnboundedReceiver;
use futures::stream::{self, StreamExt};
use influxdb2::Client;
use influxdb2::models::DataPoint;
use influxdb2::models::health::Status;
use log::{info, warn};
use serde::Deserialize;
use std::iter::zip;
use std::sync::Arc;
use std::time::Duration;

use super::receiver::{Receiver, Update};

pub struct Influxdb2Receiver {
    client: Client,
    bucket: String,
}

impl Influxdb2Receiver {
    pub async fn new(config: &Config) -> Self {
        let client = Client::new(&config.host, &config.org, &config.token);
        match client.health().await {
            Ok(health_check) => {
                if health_check.status == Status::Fail {
                    match health_check.message {
                        Some(ref message) => {
                            warn!("Influxdb server is unhealthy: {message}");
                        }
                        None => {
                            warn!("Influxdb server is unhealthy");
                        }
                    }
                } else {
                    info!(
                        "Successfully connected to Influxdb server at {}",
                        &config.host
                    );
                }
            }
            Err(err) => {
                warn!("Could not connect to Influxdb server: {err}");
            }
        }
        Self {
            client,
            bucket: config.bucket.to_owned(),
        }
    }
}

#[async_trait]
impl Receiver for Influxdb2Receiver {
    async fn run<'a>(&mut self, mut receiver: UnboundedReceiver<Arc<Update<'a>>>) {
        while let Some(update) = receiver.next().await {
            let mut points = vec![];
            for (field, value) in zip(update.fields.iter(), update.values.iter()) {
                let build = DataPoint::builder("inverter")
                    .timestamp(update.timestamp)
                    .tag("serial", update.serial.as_str())
                    .tag("group", field.group)
                    .tag("name", field.name);
                let build = if field.unit.is_empty() {
                    build
                } else {
                    build.tag("unit", field.unit)
                };
                let build = build.field("value", *value).build();
                match build {
                    Ok(value) => {
                        points.push(value);
                    }
                    Err(err) => {
                        warn!("Error building point: {err:?}");
                    }
                }
            }
            if !points.is_empty() {
                loop {
                    match self
                        .client
                        .write(self.bucket.as_str(), stream::iter(points.clone()))
                        .await
                    {
                        Ok(_) => {
                            break;
                        }
                        Err(err) => {
                            info!("Error writing to Influxdb; trying again in 5s ({err:?})");
                            task::sleep(Duration::from_secs(5)).await;
                        }
                    }
                }
            }
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default = "default_host")]
    pub host: String,
    pub org: String,
    pub token: String,
    pub bucket: String,
}

fn default_host() -> String {
    "http://localhost:8086".to_string()
}
