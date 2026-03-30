/** @type {import('tailwindcss').Config} */
export default {
  content: ["./src/**/*.{html,js,svelte,ts}"],
  theme: {
    extend: {
      colors: {
        brass: {
          50: "#faf6ec",
          100: "#f5ebd2",
          200: "#ead39d",
          300: "#ddb668",
          400: "#c78e3a",
          500: "#a96d2d",
          600: "#865525",
          700: "#643f1f",
          800: "#412915",
          900: "#241508"
        },
        ink: {
          50: "#f5f7fa",
          100: "#e9edf2",
          200: "#ced7e1",
          300: "#9aa8b8",
          400: "#627286",
          500: "#445366",
          600: "#2d394a",
          700: "#202938",
          800: "#151c27",
          900: "#0d1218"
        }
      },
      fontFamily: {
        sans: ["\"Noto Sans JP\"", "ui-sans-serif", "system-ui", "sans-serif"]
      },
      boxShadow: {
        panel: "0 20px 60px rgba(13, 18, 24, 0.10)"
      }
    }
  }
};
