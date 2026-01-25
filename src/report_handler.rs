use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt::{Debug, Display};
use std::vec::Drain;

use crate::hidreport::{Field, Report, ReportDescriptor};
use crate::usage_tables::{GenericDesktopUsage, KeyboardOrKeypadUsage, UsagePage};

#[derive(Debug, Clone)]
pub struct ReportEvent {
    pub usage_page: u16,
    pub usage: u16,
    pub value: i64,
    pub relative: bool,
}

impl Display for ReportEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let page = UsagePage::from_repr(self.usage_page);
        match page.as_ref() {
            Some(p) => f.write_str(p.into())?,
            None => write!(f, "{}", self.usage_page)?,
        }

        f.write_str(":")?;

        match page {
            Some(UsagePage::GenericDesktop) => match GenericDesktopUsage::from_repr(self.usage) {
                Some(u) => f.write_str(u.into())?,
                None => write!(f, "{}", self.usage)?,
            },
            Some(UsagePage::KeyboardOrKeypad) => {
                match KeyboardOrKeypadUsage::from_repr(self.usage as u8) {
                    Some(u) => f.write_str(u.into())?,
                    None => write!(f, "{}", self.usage)?,
                }
            }
            _ => write!(f, "{}", self.usage)?,
        }

        write!(
            f,
            ":{}:{}",
            self.value,
            if self.relative { "rel" } else { "abs" }
        )
    }
}

pub struct ReportHandler {
    pub descriptor: ReportDescriptor,
    pub total_byte_length: usize,
    pub absolutes: HashMap<(u16, u16), i64>,
    pub arrays: HashSet<(u16, u16)>,
    next_arrays: HashSet<(u16, u16)>,
    event_buffer: Vec<ReportEvent>,
}

impl ReportHandler {
    pub fn new(descriptor_bytes: &[u8]) -> Result<Self, Box<dyn Error>> {
        let descriptor = ReportDescriptor::try_from(descriptor_bytes)?;
        let mut total_byte_length = 0;
        for report in descriptor.input_reports() {
            let size = report.size_in_bytes();
            if size > total_byte_length {
                total_byte_length = size;
            }
        }
        Ok(Self {
            descriptor,
            total_byte_length,
            absolutes: HashMap::new(),
            arrays: HashSet::new(),
            next_arrays: HashSet::with_capacity(32),
            event_buffer: Vec::with_capacity(32),
        })
    }

    pub fn handle<'a>(
        &'a mut self,
        report_bytes: &[u8],
    ) -> Result<ReportEvents<'a>, Box<dyn Error>> {
        self.event_buffer.clear();
        self.next_arrays.clear();

        if let Some(report) = self.descriptor.find_input_report(report_bytes) {
            for field in report.fields() {
                match field {
                    Field::Variable(variable) => {
                        let usage_page = variable.usage.usage_page.into();
                        let usage = variable.usage.usage_id.into();
                        let value = if variable.is_signed() {
                            i64::from(variable.extract_i32(&report_bytes)?)
                        } else {
                            i64::from(variable.extract_u32(&report_bytes)?)
                        };
                        let relative = variable.is_relative;
                        if relative {
                            if value == 0 {
                                // Skip relative events where value has not changed
                                continue;
                            }
                        } else {
                            if let Some(last_value) = self.absolutes.get(&(usage_page, usage)) {
                                if &value == last_value {
                                    // Skip absolute events where value has not changed
                                    continue;
                                }
                            }
                            // Insert new value
                            self.absolutes.insert((usage_page, usage), value);
                        }
                        let ev = ReportEvent {
                            usage_page,
                            usage,
                            value,
                            relative,
                        };

                        log::trace!(
                            "var {} ({}..={})",
                            ev,
                            variable.logical_minimum,
                            variable.logical_maximum
                        );

                        self.event_buffer.push(ev);
                    }
                    Field::Array(array) => {
                        log::trace!(
                            "array range = {}..={}, count = {}",
                            array.logical_minimum,
                            array.logical_maximum,
                            array.report_count,
                        );
                        //TODO: use array.is_signed?
                        for index in array.extract_u32(&report_bytes)? {
                            log::trace!("  = {:?}", index);
                            if let Some(usage) = array.usages().get(usize::try_from(index)?) {
                                self.next_arrays.insert((
                                    u16::from(usage.usage_page),
                                    u16::from(usage.usage_id),
                                ));
                            }
                        }
                    }
                    Field::Constant(_) => {}
                }
            }
        }

        for &(usage_page, usage) in self.arrays.iter() {
            if !self.next_arrays.contains(&(usage_page, usage)) {
                // Release array items missing from next_arrays
                self.event_buffer.push(ReportEvent {
                    usage_page,
                    usage,
                    value: 0,
                    relative: false,
                });
            }
        }
        for &(usage_page, usage) in self.next_arrays.iter() {
            if !self.arrays.contains(&(usage_page, usage)) {
                // Press array items only in next_arrays
                self.event_buffer.push(ReportEvent {
                    usage_page,
                    usage,
                    value: 1,
                    relative: false,
                });
            }
        }

        std::mem::swap(&mut self.arrays, &mut self.next_arrays);

        Ok(ReportEvents {
            iter: self.event_buffer.drain(..),
        })
    }
}

pub struct ReportEvents<'a> {
    iter: Drain<'a, ReportEvent>,
}

impl<'a> Iterator for ReportEvents<'a> {
    type Item = ReportEvent;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next()
    }
}
