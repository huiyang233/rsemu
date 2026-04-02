import React from "react";

interface BadgeProps {
  children: React.ReactNode;
  color?: "green" | "red" | "yellow" | "gray" | "indigo";
}

const colorClass: Record<NonNullable<BadgeProps["color"]>, string> = {
  green:  "bg-green-900  text-green-300",
  red:    "bg-red-900    text-red-300",
  yellow: "bg-yellow-900 text-yellow-300",
  gray:   "bg-[#313244]  text-[#9399b2]",
  indigo: "bg-indigo-900 text-indigo-300",
};

export default function Badge({ children, color = "gray" }: BadgeProps) {
  return (
    <span className={`inline-block rounded px-1.5 py-0.5 text-xs font-medium ${colorClass[color]}`}>
      {children}
    </span>
  );
}
