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

use futures::prelude::*;
use futures::channel::mpsc;
use serde::Deserialize;
use serde_with::serde_as;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::MissedTickBehavior;
use tokio_modbus::prelude::Reader;
use tokio_modbus::slave::Slave;

use crate::fields::FIELDS;
use crate::receiver::Update;

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

static FIELD_MAP: &[(&'static str, &[u16])] = &[
    ("battery_charge_total", &[72, 73]),
    ("battery_discharge_total", &[74, 75]),
    ("grid_import_total", &[78, 80]),
    ("grid_frequency", &[79]),
    ("grid_export_total", &[81, 82]),
    ("load_consumption_total", &[85, 86]),
    ("inverter_temperature_dc", &[90]),
    ("inverter_temperature_ac", &[91]), // TODO: 91 is "radiator temperature" in kellerza/sunsynk
    ("pv_production_total", &[96, 97]),
    // TODO: battery_capacity?
    ("pv_voltage_1", &[109]),
    ("pv_current_1", &[110]),
    ("pv_voltage_2", &[111]),
    ("pv_current_2", &[112]),
];

pub fn create_stream(
    config: &ModbusConfig,
) -> Result<Box<dyn Stream<Item = Result<Option<Arc<Update<'static>>>, pcap::Error>> + Unpin>, Box<dyn std::error::Error>> {
    let serial_builder = tokio_serial::new(&config.device, config.baud);
    let serial_stream = tokio_serial::SerialStream::open(&serial_builder)?;
    let (mut sender, receiver) = mpsc::channel(1);
    let interval = config.interval;
    let modbus_id = config.modbus_id;
    tokio::spawn(async move {
        // TODO: error handling
        let mut ctx = tokio_modbus::client::rtu::connect_slave(serial_stream, Slave(modbus_id)).await.unwrap();
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
            let mut values = vec![0.0; FIELDS.len()];
            for (id, regs) in FIELD_MAP.iter() {
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
                let pos = FIELDS.iter().position(|x| x.id == *id).unwrap();
                let field = &FIELDS[pos];
                let value = (raw as f64) * field.scale + field.bias;
                values[pos] = value;
            }
            let now = chrono::Utc::now();
            let update = Update::new(now.timestamp_nanos(), serial, FIELDS, values);
            // TODO: Handle error from send
            sender.send(Ok(Some(Arc::new(update)))).await.unwrap();
        }
    });
    Ok(Box::new(receiver))
}
