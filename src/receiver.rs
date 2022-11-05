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

use super::fields::Field;

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

impl<'a> Update<'a> {
    pub fn new(
        timestamp: i64,
        serial: impl Into<String>,
        fields: &'a [Field<'a>],
        values: Vec<f64>,
    ) -> Self {
        Update {
            timestamp,
            serial: serial.into(),
            fields,
            values,
        }
    }
}
