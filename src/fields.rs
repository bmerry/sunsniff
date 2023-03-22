/* Copyright 2022-2023 Bruce Merry
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
    pub fn from_u16s(&self, parts: impl IntoIterator<Item = u16>) -> f64 {
        let mut raw: i64 = 0;
        let mut shift: u32 = 0;
        for part in parts {
            raw += (part as i64) << shift;
            shift += 16;
        }
        let wrap: i64 = 1i64 << (shift - 1);
        // Convert to signed (TODO: most registers are actually unsigned)
        if raw >= wrap {
            raw -= 2 * wrap;
        }
        (raw as f64) * self.scale + self.bias
    }
}
