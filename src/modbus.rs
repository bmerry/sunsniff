/* Copyright 2023 Bruce Merry
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

use futures::channel::mpsc;
use futures::prelude::*;
use log::{error, info};
use serde::Deserialize;
use serde_with::serde_as;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::MissedTickBehavior;
use tokio_modbus::client::Context;
use tokio_modbus::prelude::Reader;
use tokio_modbus::slave::Slave;

use crate::receiver::{Update, UpdateStream};

/// Structure corresponding to the `[modbus]` section of the configuration file.
#[serde_as]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModbusConfig {
    device: String,
    #[serde_as(as = "serde_with::DurationSecondsWithFrac<f64>")]
    interval: Duration,
    #[serde(default = "default_baud")]
    baud: u32,
    #[serde(default = "default_modbus_id")]
    modbus_id: u8,
}

fn default_baud() -> u32 {
    9600
}

fn default_modbus_id() -> u8 {
    1
}

async fn read_values(ctx: &mut Context) -> Result<Vec<f64>, std::io::Error> {
    let mut values = Vec::with_capacity(FIELDS.len());
    let mut parts = [0u16; 2];
    for (field, regs) in FIELDS.iter().zip(REGISTERS.iter()) {
        for (i, reg) in regs.iter().enumerate() {
            // TODO: better error handling
            parts[i] = ctx.read_holding_registers(*reg, 1).await?[0];
        }
        let value = field.from_u16s(parts[..regs.len()].iter().cloned());
        values.push(value);
    }
    Ok(values)
}

pub async fn create_stream(
    config: &ModbusConfig,
) -> Result<UpdateStream, Box<dyn std::error::Error>> {
    let serial_builder = tokio_serial::new(&config.device, config.baud);
    let serial_stream = tokio_serial::SerialStream::open(&serial_builder)?;
    let (mut sender, receiver) = mpsc::channel(1);
    let interval = config.interval;
    let modbus_id = config.modbus_id;
    let mut ctx = tokio_modbus::client::rtu::connect_slave(serial_stream, Slave(modbus_id)).await?;
    let serial_words = ctx.read_holding_registers(3, 5).await?;
    let mut serial_bytes = [0u8; 10];
    for i in 0..5 {
        let bytes = serial_words[i].to_be_bytes();
        serial_bytes[2 * i] = bytes[0];
        serial_bytes[2 * i + 1] = bytes[1];
    }
    let serial = std::str::from_utf8(&serial_bytes)?.to_owned();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(interval);
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            match read_values(&mut ctx).await {
                Err(err) => {
                    error!("Failed to read values from modbus: {err:?}");
                }
                Ok(values) => {
                    info!("Received a set of values from modbus");
                    let now = chrono::Utc::now();
                    let update = Update::new(now.timestamp_nanos(), &serial, FIELDS, values);
                    // TODO: Handle error from send
                    sender.send(Arc::new(update)).await.unwrap();
                }
            }
        }
    });
    Ok(Box::pin(receiver))
}

include!(concat!(env!("OUT_DIR"), "/modbus_fields.rs"));
