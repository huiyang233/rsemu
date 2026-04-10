import React from "react";

export default function LabelInput({
  label,
  value,
  onChange,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
}) {
  return (
    <label className="flex flex-col gap-1 text-xs text-[#a6adc8]">
      {label}
      <input
        type="text"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        className="bg-[#2a2a3e] border border-[#3a3a5e] rounded px-2 py-1.5 text-sm text-[#cdd6f4] focus:outline-none focus:border-indigo-500"
      />
    </label>
  );
}
