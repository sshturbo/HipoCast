/** @type {import('tailwindcss').Config} */
export default {
    darkMode: 'class',
    content: [
        "./index.html",
        "./src/**/*.{js,ts,jsx,tsx}",
    ],
    theme: {
        extend: {
            colors: {
                border: "hsl(217.2 32.6% 17.5%)",
                input: "hsl(217.2 32.6% 17.5%)",
                ring: "hsl(263.4 70% 50.4%)",
                background: "hsl(222.2 84% 4.9%)",
                foreground: "hsl(210 40% 98%)",
                primary: {
                    DEFAULT: "hsl(263.4 70% 50.4%)",
                    foreground: "hsl(210 40% 98%)",
                },
                secondary: {
                    DEFAULT: "hsl(217.2 32.6% 17.5%)",
                    foreground: "hsl(210 40% 98%)",
                },
                destructive: {
                    DEFAULT: "hsl(0 62.8% 30.6%)",
                    foreground: "hsl(210 40% 98%)",
                },
                muted: {
                    DEFAULT: "hsl(217.2 32.6% 17.5%)",
                    foreground: "hsl(215 20.2% 65.1%)",
                },
                accent: {
                    DEFAULT: "hsl(217.2 32.6% 17.5%)",
                    foreground: "hsl(210 40% 98%)",
                },
                card: {
                    DEFAULT: "hsl(222.2 84% 6.5%)",
                    foreground: "hsl(210 40% 98%)",
                },
            },
            borderRadius: {
                lg: "0.75rem",
                md: "0.5rem",
                sm: "0.25rem",
            },
        },
    },
    plugins: [],
}
