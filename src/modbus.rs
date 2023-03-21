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
use serde::Deserialize;
use serde_with::serde_as;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::MissedTickBehavior;
use tokio_modbus::prelude::Reader;
use tokio_modbus::slave::Slave;

use crate::receiver::{Update, UpdateItem};

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

pub fn create_stream(
    config: &ModbusConfig,
) -> Result<Box<dyn Stream<Item = UpdateItem> + Unpin>, Box<dyn std::error::Error>> {
    let serial_builder = tokio_serial::new(&config.device, config.baud);
    let serial_stream = tokio_serial::SerialStream::open(&serial_builder)?;
    let (mut sender, receiver) = mpsc::channel(1);
    let interval = config.interval;
    let modbus_id = config.modbus_id;
    tokio::spawn(async move {
        // TODO: error handling
        let mut ctx = tokio_modbus::client::rtu::connect_slave(serial_stream, Slave(modbus_id))
            .await
            .unwrap();
        let mut interval = tokio::time::interval(interval);
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
        // TODO: error handling
        let serial_words = ctx.read_holding_registers(3, 5).await.unwrap();
        let mut serial_bytes = [0u8; 10];
        for i in 0..5 {
            let bytes = serial_words[i].to_be_bytes();
            serial_bytes[2 * i] = bytes[0];
            serial_bytes[2 * i + 1] = bytes[1];
        }
        // TODO: error handling
        let serial = std::str::from_utf8(&serial_bytes).unwrap();
        loop {
            interval.tick().await;
            let mut values = Vec::with_capacity(FIELDS.len());
            for (field, regs) in FIELDS.iter().zip(REGISTERS.iter()) {
                let mut raw: i64 = 0;
                let mut shift: u32 = 0;
                for reg in regs.iter() {
                    // TODO: error handling
                    let reg_value = ctx.read_holding_registers(*reg, 1).await.unwrap()[0];
                    raw += (reg_value as i64) << shift;
                    shift += 16;
                }
                let wrap: i64 = 1i64 << (shift - 1);
                // Convert to signed (TODO: most registers are actually unsigned)
                if raw >= wrap {
                    raw -= 2 * wrap;
                }
                // TODO: optimise this search (ideally at compile time)
                let value = (raw as f64) * field.scale + field.bias;
                values.push(value);
            }
            let now = chrono::Utc::now();
            let update = Update::new(now.timestamp_nanos(), serial, FIELDS, values);
            // TODO: Handle error from send
            sender.send(Ok(Some(Arc::new(update)))).await.unwrap();
        }
    });
    Ok(Box::new(receiver))
}

include!(concat!(env!("OUT_DIR"), "/modbus_fields.rs"));
