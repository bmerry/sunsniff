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
    Time,
    Voltage,
    Unitless,
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
    /// Indices of other fields to sum to get this field
    pub sum_of: &'a [usize],
}

impl Field<'_> {
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
        // Special handling for time fields: HH:MM is encoded as HH*100+MM.
        if self.field_type == FieldType::Time {
            let h = raw / 100;
            let m = raw % 100;
            raw = h * 60 + m;
        }
        (raw as f64) * self.scale + self.bias
    }

    pub fn from_sum(&self, values: &[f64]) -> f64 {
        self.sum_of.iter().map(|idx| values[*idx]).sum()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use assert_approx_eq::assert_approx_eq;

    fn field() -> Field<'static> {
        Field {
            field_type: FieldType::Energy,
            group: "Grid",
            name: "Total import",
            id: "grid_import",
            scale: 0.1,
            bias: -10.0, // Not realistic, but useful to test the feature
            unit: "kWh",
            sum_of: &[1, 2],
        }
    }

    #[test]
    fn test_from_u16s_one() {
        let f = field();
        assert_approx_eq!(f.from_u16s([12345]), 1224.5);
        assert_approx_eq!(f.from_u16s([55536]), -1010.0);
    }

    #[test]
    fn test_from_u16s_two() {
        let f = field();
        assert_approx_eq!(f.from_u16s([12345, 4321]), 28319330.1);
        assert_approx_eq!(f.from_u16s([55536, 4321]), 28323649.2);
        assert_approx_eq!(f.from_u16s([55536, 55536]), -65530456.4);
    }

    #[test]
    fn test_from_sum() {
        let f = field();
        let values = [2.0, 3.0, 4.0];
        assert_eq!(f.from_sum(&values), 7.0);
    }
}
