/** @type {import('tailwindcss').Config} */
module.exports = {
  content: [
    "./crates/loriot-web/src/**/*.rs",
    "./crates/loriot-web/**/*.html",
    "./assets/**/*.html",
  ],
  safelist: [
    // DaisyUI components we use
    'card', 'card-body', 'card-title', 'card-actions',
    'stat', 'stats', 'stat-title', 'stat-value', 'stat-desc', 'stat-figure',
    'badge', 'badge-success', 'badge-warning', 'badge-error', 'badge-info', 'badge-primary',
    'btn', 'btn-primary', 'btn-ghost', 'btn-outline',
    'navbar', 'navbar-start', 'navbar-center', 'navbar-end',
    'alert', 'alert-info', 'alert-warning', 'alert-error',
    'menu', 'menu-horizontal',
    'select', 'select-bordered', 'select-sm',
    'container', 'shadow', 'shadow-xl',
    'animate-pulse',
    // Layout utilities
    'grid', 'grid-cols-1', 'md:grid-cols-2', 'lg:grid-cols-3',
    'flex', 'justify-between', 'items-center',
    'gap-4', 'p-4', 'mb-4', 'mb-6', 'mt-8', 'ml-4',
    'w-full', 'w-8', 'h-8', 'min-h-screen',
    // Text utilities
    'text-xl', 'text-2xl', 'text-3xl', 'font-bold',
    'text-primary', 'text-secondary', 'text-accent', 'text-warning', 'text-error',
    // Background
    'bg-base-100', 'bg-base-200', 'bg-base-300',
    'rounded-lg',
  ],
  theme: {
    extend: {},
  },
  plugins: [require("daisyui")],
  daisyui: {
    themes: ["corporate", "business", "light", "dark"],
    darkTheme: "business",
    base: true,
    styled: true,
    utils: true,
    prefix: "",
    logs: true,
    themeRoot: ":root",
  },
}
