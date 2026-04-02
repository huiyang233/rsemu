import type { Config } from "tailwindcss";

export default {
  content: ["./index.html", "./src/**/*.{js,ts,jsx,tsx}"],
  theme: {
    extend: {
      colors: {
        surface: {
          DEFAULT: "#1e1e2e",
          raised: "#2a2a3e",
          border: "#3a3a5e",
        },
      },
    },
  },
  plugins: [],
} satisfies Config;
