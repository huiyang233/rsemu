import React from "react";

type Variant = "primary" | "secondary" | "danger" | "ghost";
type Size = "sm" | "md";

interface ButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: Variant;
  size?: Size;
}

const variantClass: Record<Variant, string> = {
  primary:   "bg-indigo-600 hover:bg-indigo-500 text-white",
  secondary: "bg-[#2a2a3e] hover:bg-[#3a3a5e] text-[#cdd6f4] border border-[#3a3a5e]",
  danger:    "bg-red-700 hover:bg-red-600 text-white",
  ghost:     "hover:bg-[#2a2a3e] text-[#cdd6f4]",
};

const sizeClass: Record<Size, string> = {
  sm: "px-2 py-1 text-xs",
  md: "px-3 py-1.5 text-sm",
};

export default function Button({
  variant = "secondary",
  size = "md",
  className = "",
  children,
  ...props
}: ButtonProps) {
  return (
    <button
      className={`
        inline-flex items-center gap-1.5 rounded font-medium
        transition-colors disabled:opacity-40 disabled:cursor-not-allowed
        ${variantClass[variant]} ${sizeClass[size]} ${className}
      `}
      {...props}
    >
      {children}
    </button>
  );
}
