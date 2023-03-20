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

use std::ops::Range;

/// Type of quantity stored in a field
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FieldType {
    Charge,
    Current,
    Energy,
    Frequency,
    Power,
    StateOfCharge,
    Temperature,
    Voltage,
}

/// Static description of a field in the data
#[derive(Debug)]
pub struct Field<'a> {
    pub field_type: FieldType,
    /// Byte offset within the TCP payload
    pub offset: usize,
    pub group: &'a str,
    pub name: &'a str,
    pub id: &'a str,
    /// Amount by which to scale the raw integer value
    pub scale: f64,
    /// Amount to add to the value, after scaling
    pub bias: f64,
    pub unit: &'a str,
}

impl<'a> Field<'a> {
    /// Create a field representing power, with a custom name
    pub const fn power_name(offset: usize, group: &'a str, name: &'a str, id: &'a str) -> Self {
        Field {
            field_type: FieldType::Power,
            offset,
            group,
            name,
            id,
            scale: 1.0,
            bias: 0.0,
            unit: "W",
        }
    }

    /// Create a field representing power
    pub const fn power(offset: usize, group: &'a str, id: &'a str) -> Self {
        Field::power_name(offset, group, "Power", id)
    }

    /// Create a field representing voltage, with a custom name
    pub const fn voltage_name(
        offset: usize,
        group: &'a str,
        name: &'a str,
        id: &'a str,
        scale: f64,
    ) -> Self {
        Field {
            field_type: FieldType::Voltage,
            offset,
            group,
            name,
            id,
            scale,
            bias: 0.0,
            unit: "V",
        }
    }

    /// Create a field representing voltage
    pub const fn voltage(offset: usize, group: &'a str, id: &'a str, scale: f64) -> Self {
        Field::voltage_name(offset, group, "Voltage", id, scale)
    }

    /// Create a field representing current, with a custom name
    pub const fn current_name(
        offset: usize,
        group: &'a str,
        name: &'a str,
        id: &'a str,
        scale: f64,
    ) -> Self {
        Field {
            field_type: FieldType::Current,
            offset,
            group,
            name,
            id,
            scale,
            bias: 0.0,
            unit: "A",
        }
    }

    /// Create a field representing current
    pub const fn current(offset: usize, group: &'a str, id: &'a str, scale: f64) -> Self {
        Field::current_name(offset, group, "Current", id, scale)
    }

    /// Create a field representing temperature, with a custom name
    pub const fn temperature_name(
        offset: usize,
        group: &'a str,
        name: &'a str,
        id: &'a str,
    ) -> Self {
        Field {
            field_type: FieldType::Temperature,
            offset,
            group,
            name,
            id,
            scale: 0.1,
            bias: -100.0,
            unit: "°C",
        }
    }

    /// Create a field representing temperature
    pub const fn temperature(offset: usize, group: &'a str, id: &'a str) -> Self {
        Field::temperature_name(offset, group, "Temperature", id)
    }

    /// Create a field representing frequency
    pub const fn frequency(offset: usize, group: &'a str, id: &'a str) -> Self {
        Field {
            field_type: FieldType::Frequency,
            offset,
            group,
            name: "Frequency",
            id,
            scale: 0.01,
            bias: 0.0,
            unit: "Hz",
        }
    }

    /// Create a field representing energy
    pub const fn energy(offset: usize, group: &'a str, name: &'a str, id: &'a str) -> Self {
        // TODO: these are probably 32-bit values, but more investigation is
        // needed to figure out where the high bits live.
        Field {
            field_type: FieldType::Energy,
            offset,
            group,
            name,
            id,
            scale: 0.1,
            bias: 0.0,
            unit: "kWh",
        }
    }

    /// Create a field representing charge
    pub const fn charge(offset: usize, group: &'a str, name: &'a str, id: &'a str) -> Self {
        Field {
            field_type: FieldType::Charge,
            offset,
            group,
            name,
            id,
            scale: 1.0,
            bias: 0.0,
            unit: "Ah",
        }
    }

    /// Create a field representing state of charge
    pub const fn state_of_charge(offset: usize, group: &'a str, id: &'a str) -> Self {
        Field {
            field_type: FieldType::StateOfCharge,
            offset,
            group,
            name: "SOC",
            id,
            scale: 1.0,
            bias: 0.0,
            unit: "%",
        }
    }
}

/// Expected length of the packet (TCP payload)
pub const MAGIC_LENGTH: usize = 292;
/// Expected first byte of the packet
pub const MAGIC_HEADER: u8 = 0xa5;
/// Offsets containing the inverter serial number
pub const SERIAL_RANGE: Range<usize> = 11..21;
/// Offset at which the timestamp is located
pub const DATETIME_OFFSET: usize = 37;

include!(concat!(env!("OUT_DIR"), "/field_definitions.rs"));
