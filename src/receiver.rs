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
use std::sync::Arc;

#[derive(Debug)]
pub struct Field<'a> {
    pub offset: usize,
    pub group: &'a str,
    pub name: &'a str,
    pub id: &'a str,
    pub scale: f64,
    pub bias: f64,
    pub unit: &'a str,
}

#[derive(Debug)]
pub struct Update<'a> {
    pub timestamp: i64, // Nanoseconds since UNIX epoch
    pub serial: String,
    pub fields: &'a [Field<'a>],
    pub values: Vec<f64>,
}

#[async_trait]
pub trait Receiver {
    async fn run<'a>(&mut self, receiver: UnboundedReceiver<Arc<Update<'a>>>);
}

impl<'a> Field<'a> {
    pub const fn new(
        offset: usize,
        group: &'a str,
        name: &'a str,
        id: &'a str,
        scale: f64,
        bias: f64,
        unit: &'a str,
    ) -> Self {
        return Field {
            offset,
            group,
            name,
            id,
            scale,
            bias,
            unit,
        };
    }

    pub const fn power(offset: usize, group: &'a str, id: &'a str) -> Self {
        return Field::new(offset, group, "Power", id, 1.0, 0.0, "W");
    }

    pub const fn voltage(offset: usize, group: &'a str, id: &'a str) -> Self {
        return Field::new(offset, group, "Voltage", id, 0.1, 0.0, "V");
    }

    pub const fn current(offset: usize, group: &'a str, id: &'a str) -> Self {
        return Field::new(offset, group, "Current", id, 0.01, 0.0, "A");
    }

    pub const fn temperature_name(
        offset: usize,
        group: &'a str,
        name: &'a str,
        id: &'a str,
    ) -> Self {
        return Field::new(offset, group, name, id, 0.1, -100.0, "°C");
    }

    pub const fn temperature(offset: usize, group: &'a str, id: &'a str) -> Self {
        return Field::temperature_name(offset, group, "Temperature", id);
    }

    pub const fn frequency(offset: usize, group: &'a str, id: &'a str) -> Self {
        return Field::new(offset, group, "Frequency", id, 0.01, 0.0, "Hz");
    }

    pub const fn energy(offset: usize, group: &'a str, name: &'a str, id: &'a str) -> Self {
        // TODO: these are probably 32-bit values, but more investigation is
        // needed to figure out where the high bits live.
        return Field::new(offset, group, name, id, 0.1, 0.0, "kWh");
    }
}

impl<'a> Update<'a> {
    pub fn new(
        timestamp: i64,
        serial: impl Into<String>,
        fields: &'a [Field<'a>],
        values: Vec<f64>,
    ) -> Update<'a> {
        return Update {
            timestamp,
            serial: serial.into(),
            fields,
            values,
        };
    }
}
