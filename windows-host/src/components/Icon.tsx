interface IconProps {
  kind: "wave" | "pc" | "phone" | "settings" | "chart" | "sound";
}

const paths: Record<IconProps["kind"], string> = {
  wave: "M3 10v4m4-8v12m5-16v20m5-16v12m4-8v4",
  pc: "M3 4h18v13H3zM8 21h8m-4-4v4",
  phone: "M7 2h10v20H7zM11 18h2",
  settings: "M4 7h16M4 17h16M8 4v6m8 4v6",
  chart: "M4 20V10m8 10V4m8 16v-7",
  sound: "M4 9h4l5-4v14l-5-4H4zM17 8q5 4 0 8",
};

export function Icon({ kind }: IconProps) {
  return (
    <svg
      width="22"
      height="22"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.6"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d={paths[kind]} />
    </svg>
  );
}
