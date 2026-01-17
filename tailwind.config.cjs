/** @type {import('tailwindcss').Config} */
module.exports = {
  content: ["./src/**/*.{html,js,svelte,ts}"],
  theme: {
    extend: {
      fontFamily: {
        display: ['"Space Grotesk"', "system-ui", "sans-serif"],
        body: ['"IBM Plex Sans"', "system-ui", "sans-serif"],
        mono: ['"IBM Plex Mono"', "ui-monospace", "SFMono-Regular", "monospace"],
      },
      colors: {
        ink: {
          950: "#0c1116",
          900: "#121922",
          800: "#19212c",
          700: "#253041",
          600: "#2f3d52",
        },
        sun: {
          50: "#fff8ec",
          100: "#ffefd2",
          200: "#ffdca1",
          300: "#ffc36c",
          400: "#ffad43",
          500: "#f78c1f",
          600: "#dd6d0f",
          700: "#b8510c",
        },
        tide: {
          50: "#eef6ff",
          100: "#d9eaff",
          200: "#b6d2ff",
          300: "#88b2ff",
          400: "#5d8dff",
          500: "#3a6bff",
          600: "#2d51e3",
          700: "#223cc2",
        },
      },
      boxShadow: {
        glow: "0 30px 80px -40px rgba(58, 107, 255, 0.6)",
      },
    },
  },
  plugins: [require("@tailwindcss/forms")],
};
