import React from "react";

interface SelectProps extends React.SelectHTMLAttributes<HTMLSelectElement> {
  label?: string;
}

export default function Select({ label, className = "", children, ...props }: SelectProps) {
  return (
    <label className="flex flex-col gap-1 text-xs text-[#a6adc8]">
      {label && <span>{label}</span>}
      <select
        className={`
          bg-[#2a2a3e] border border-[#3a3a5e] rounded px-2 py-1.5
          text-[#cdd6f4] text-sm focus:outline-none focus:border-indigo-500
          cursor-pointer ${className}
        `}
        {...props}
      >
        {children}
      </select>
    </label>
  );
}
