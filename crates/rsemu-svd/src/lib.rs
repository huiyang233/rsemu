mod model;
mod parser;
mod xml;

pub use model::SvdDevice;
pub use parser::parse_svd;

#[cfg(test)]
mod tests {
    use crate::parse_svd;

    #[test]
    fn parse_minimal_svd() {
        let xml = r#"
        <device>
          <name>STM32F103</name>
          <peripherals>
            <peripheral>
              <name>GPIOA</name>
              <baseAddress>0x40010800</baseAddress>
              <registers>
                <register>
                  <name>CRL</name>
                  <addressOffset>0x0</addressOffset>
                  <size>32</size>
                  <resetValue>0x44444444</resetValue>
                </register>
              </registers>
            </peripheral>
          </peripherals>
        </device>
        "#;

        let device = parse_svd(xml).expect("svd should parse");
        assert_eq!(device.name, "STM32F103");
        assert_eq!(device.peripherals.len(), 1);
        assert_eq!(device.peripherals[0].registers[0].address, 0x4001_0800);
    }
}
