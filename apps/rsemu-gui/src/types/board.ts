export interface BusPeripheralInfo {
  name: string;
  base: number;
}

export interface BoardInfo {
  id: string;
  name: string;
  description: string;
  flash_kb: number;
  ram_kb: number;
  gpio_ports: string[];
  spi_peripherals: BusPeripheralInfo[];
  i2c_peripherals: BusPeripheralInfo[];
  usart_peripherals: BusPeripheralInfo[];
  fsmc_peripherals: BusPeripheralInfo[];
}
