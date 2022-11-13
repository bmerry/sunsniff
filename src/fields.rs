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

#[derive(Debug)]
pub struct Field<'a> {
    pub field_type: FieldType,
    pub offset: usize,
    pub group: &'a str,
    pub name: &'a str,
    pub id: &'a str,
    pub scale: f64,
    pub bias: f64,
    pub unit: &'a str,
}

impl<'a> Field<'a> {
    pub const fn power_name(offset: usize, group: &'a str, name: &'a str, id: &'a str) -> Self {
        Field {
            field_type: FieldType::Power,
            offset,
            group,
            name,
            id,
            scale: 1.0,
            bias: 0.0,
            unit: "W"
        }
    }

    pub const fn power(offset: usize, group: &'a str, id: &'a str) -> Self {
        Field::power_name(offset, group, "Power", id)
    }

    pub const fn voltage_name(offset: usize, group: &'a str, name: &'a str, id: &'a str, scale: f64) -> Self {
        Field {
            field_type: FieldType::Voltage,
            offset,
            group,
            name,
            id,
            scale,
            bias: 0.0,
            unit: "V"
        }
    }

    pub const fn voltage(offset: usize, group: &'a str, id: &'a str, scale: f64) -> Self {
        Field::voltage_name(offset, group, "Voltage", id, scale)
    }

    pub const fn current_name(offset: usize, group: &'a str, name: &'a str, id: &'a str, scale: f64) -> Self {
        Field {
            field_type: FieldType::Current,
            offset,
            group,
            name,
            id,
            scale,
            bias: 0.0,
            unit: "A"
        }
    }

    pub const fn current(offset: usize, group: &'a str, id: &'a str, scale: f64) -> Self {
        Field::current_name(offset, group, "Current", id, scale)
    }

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
            unit: "°C"
        }
    }

    pub const fn temperature(offset: usize, group: &'a str, id: &'a str) -> Self {
        Field::temperature_name(offset, group, "Temperature", id)
    }

    pub const fn frequency(offset: usize, group: &'a str, id: &'a str) -> Self {
        Field {
            field_type: FieldType::Frequency,
            offset,
            group,
            name: "Frequency",
            id,
            scale: 0.01,
            bias: 0.0,
            unit: "Hz"
        }
    }

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
            unit: "kWh"
        }
    }

    pub const fn charge(offset: usize, group: &'a str, name: &'a str, id: &'a str) -> Self {
        Field {
            field_type: FieldType::Charge,
            offset,
            group,
            name,
            id,
            scale: 1.0,
            bias: 0.0,
            unit: "Ah"
        }
    }

    pub const fn state_of_charge(offset: usize, group: &'a str, id: &'a str) -> Self {
        Field {
            field_type: FieldType::StateOfCharge,
            offset,
            group,
            name: "SOC",
            id,
            scale: 1.0,
            bias: 0.0,
            unit: "%"
        }
    }
}

pub const MAGIC_LENGTH: usize = 292;
pub const MAGIC_HEADER: u8 = 0xa5; // First byte in the packet
pub const SERIAL_RANGE: Range<usize> = 11..21;
pub const DATETIME_OFFSET: usize = 37;
pub const FIELDS: &[Field] = &[
    Field::energy(70, "Battery", "Total charge", "battery_charge_total"),
    Field::energy(74, "Battery", "Total discharge", "battery_discharge_total"),
    Field::energy(82, "Grid", "Total import", "grid_import_total"),
    Field::frequency(84, "Grid", "grid_frequency"),
    Field::energy(88, "Grid", "Total export", "grid_export_total"), // Might also be 112
    Field::energy(96, "Load", "Total consumption", "load_consumption_total"),
    Field::temperature_name(106, "Inverter", "DC Temperature", "inverter_temperature_dc"),
    Field::temperature_name(108, "Inverter", "AC Temperature", "inverter_temperature_ac"),
    Field::energy(118, "PV", "Total production", "pv_production_total"),
    Field::charge(140, "Battery", "Capacity", "battery_capacity"),
    Field::voltage_name(144, "PV", "Voltage 1", "pv_voltage 1", 0.1),
    Field::current_name(146, "PV", "Current 1", "pv_current_1", 0.1),
    Field::voltage_name(148, "PV", "Voltage 2", "pv_voltage_2", 0.1),
    Field::current_name(150, "PV", "Current 2", "pv_current_2", 0.1),
    Field::voltage(176, "Grid", "grid_voltage", 0.1), // Might also be 180
    Field::voltage(184, "Load", "load_voltage", 0.1), // Might also be 188
    Field::current(196, "Grid", "grid_current", 0.01),
    Field::current(204, "Load", "load_current", 0.01),
    Field::power_name(210, "Grid", "Power L1", "grid_power_l1"),
    Field::power(216, "Grid", "grid_power"), // Might also be 214 or 220
    Field::power(222, "Inverter", "inverter_power"), // Might also be 226
    Field::power(228, "Load", "load_power"), // Or 232 (one is P-load, the other P-Load-L1)
    Field::temperature(240, "Battery", "battery_temperature"),
    Field::voltage(242, "Battery", "battery_voltage", 0.01),
    Field::state_of_charge(244, "Battery", "battery_soc"),
    Field::power(248, "PV", "pv_power"),
    Field::power(256, "Battery", "battery_power"),
    Field::current(258, "Battery", "battery_current", 0.01),
    Field::frequency(260, "Load", "load_frequency"), // Might also be 262
    Field::voltage_name(276, "BMS", "Charge Voltage", "bms_charge_voltage", 0.01),
    Field::current_name(280, "BMS", "Charge Limit Current", "bms_charge_limit_current", 1.0),
    Field::current_name(282, "BMS", "Discharge Limit Current", "bms_discharge_limit_current", 1.0),
    Field::voltage(286, "BMS", "bms_voltage", 0.01),
    Field::current(288, "BMS", "bms_current", 1.0),
    Field::temperature(290, "BMS", "bms_temperature"),
];
