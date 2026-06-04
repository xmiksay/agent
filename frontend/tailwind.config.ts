import type { Config } from "tailwindcss";

export default {
  content: ["./index.html", "./src/**/*.{vue,ts,tsx}"],
  theme: {
    extend: {
      colors: {
        ink: {
          50: "#f5f6f7",
          900: "#0f172a",
        },
      },
    },
  },
} satisfies Config;
