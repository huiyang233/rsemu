import React from "react";
import type { PinConfigProps } from "../peripherals/registry";
import { typeToPinConfig, getPeripheralDef } from "../peripherals/registry";

interface Props {
  item: PinConfigProps["item"];
  board: PinConfigProps["board"];
  onSave: PinConfigProps["onSave"];
  onClose: PinConfigProps["onClose"];
}

export default function PinConfigPanel({ item, board, onSave, onClose }: Props) {
  const PinConfig = typeToPinConfig.get(item.type);
  if (!PinConfig) return null;
  return <PinConfig item={item} board={board} onSave={onSave} onClose={onClose} />;
}
