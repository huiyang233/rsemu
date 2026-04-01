use std::collections::BTreeMap;

use rsemu_core::{PeripheralSpec, RegisterSpec};

use crate::model::SvdDevice;
use crate::xml::{
    collect_tag_blocks, collect_tag_blocks_with_attrs, first_tag_block, first_tag_text,
};

pub fn parse_svd(xml: &str) -> Result<SvdDevice, String> {
    let device_block = first_tag_block(xml, "device").unwrap_or(xml);
    let name = first_tag_text(device_block, "name")
        .unwrap_or("unknown-device")
        .to_string();
    let peripherals_block = first_tag_block(device_block, "peripherals")
        .ok_or_else(|| "missing <peripherals> block in SVD".to_string())?;

    let peripheral_blocks = collect_tag_blocks_with_attrs(peripherals_block, "peripheral");
    if peripheral_blocks.is_empty() {
        return Err("SVD does not define any <peripheral>".to_string());
    }

    let mut peripherals = peripheral_blocks
        .into_iter()
        .map(|block| parse_peripheral(block.content, parse_attr(block.attrs, "derivedFrom")))
        .collect::<Result<Vec<_>, _>>()?;

    let by_name: BTreeMap<String, usize> = peripherals
        .iter()
        .enumerate()
        .map(|(idx, p)| (p.spec.name.clone(), idx))
        .collect();

    for idx in 0..peripherals.len() {
        if !peripherals[idx].spec.registers.is_empty() {
            continue;
        }
        let Some(parent_name) = peripherals[idx].derived_from.clone() else {
            continue;
        };
        let Some(&parent_idx) = by_name.get(&parent_name) else {
            continue;
        };
        let parent = peripherals[parent_idx].spec.clone();
        if parent.registers.is_empty() {
            continue;
        }
        let base = peripherals[idx].spec.base_address;
        let parent_base = parent.base_address;
        peripherals[idx].spec.registers = parent
            .registers
            .into_iter()
            .map(|mut reg| {
                reg.address = base + (reg.address - parent_base);
                reg
            })
            .collect();
    }

    let peripherals = peripherals.into_iter().map(|p| p.spec).collect();

    Ok(SvdDevice { name, peripherals })
}

#[derive(Clone, Debug)]
struct ParsedPeripheral {
    spec: PeripheralSpec,
    derived_from: Option<String>,
}

fn parse_peripheral(xml: &str, derived_from: Option<String>) -> Result<ParsedPeripheral, String> {
    let name = first_tag_text(xml, "name")
        .ok_or_else(|| "peripheral missing <name>".to_string())?
        .to_string();
    let base_address = parse_u64(
        first_tag_text(xml, "baseAddress")
            .ok_or_else(|| format!("peripheral {name} missing <baseAddress>"))?,
    )?;

    let registers = first_tag_block(xml, "registers")
        .map(|block| collect_tag_blocks(block, "register"))
        .unwrap_or_default()
        .into_iter()
        .map(|register_xml| parse_register(register_xml, base_address))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(ParsedPeripheral {
        spec: PeripheralSpec {
            name,
            base_address,
            registers,
        },
        derived_from,
    })
}

fn parse_register(xml: &str, base_address: u64) -> Result<RegisterSpec, String> {
    let name = first_tag_text(xml, "name")
        .ok_or_else(|| "register missing <name>".to_string())?
        .to_string();
    let offset = parse_u64(
        first_tag_text(xml, "addressOffset")
            .ok_or_else(|| format!("register {name} missing <addressOffset>"))?,
    )?;
    let width_bits = first_tag_text(xml, "size")
        .map(parse_u32)
        .transpose()?
        .unwrap_or(32);
    let reset_value = first_tag_text(xml, "resetValue")
        .map(parse_u64)
        .transpose()?
        .unwrap_or(0);

    Ok(RegisterSpec {
        name,
        address: base_address + offset,
        width_bits,
        reset_value,
    })
}

fn parse_u32(input: &str) -> Result<u32, String> {
    parse_u64(input).map(|value| value as u32)
}

fn parse_u64(input: &str) -> Result<u64, String> {
    let trimmed = input.trim();
    if let Some(hex) = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
    {
        u64::from_str_radix(hex, 16).map_err(|err| err.to_string())
    } else {
        trimmed.parse::<u64>().map_err(|err| err.to_string())
    }
}

fn parse_attr(attrs: &str, key: &str) -> Option<String> {
    let needle = format!(r#"{key}=""#);
    let start = attrs.find(&needle)? + needle.len();
    let tail = &attrs[start..];
    let end = tail.find('"')?;
    Some(tail[..end].to_string())
}
