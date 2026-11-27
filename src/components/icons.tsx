import { useId, type SVGProps } from "react";
import type { PlatformKind } from "@/types/rpa";

export interface IconProps extends SVGProps<SVGSVGElement> {
  size?: number;
}

const baseProps = (size = 16, className?: string): SVGProps<SVGSVGElement> => ({
  width: size,
  height: size,
  viewBox: "0 0 24 24",
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.75,
  strokeLinecap: "round",
  strokeLinejoin: "round",
  "aria-hidden": true,
  className: className ? `of-icon ${className}` : "of-icon",
});

/** OfferFlow 定制几何品牌标志（O 环 + 45° 上升流箭头，内置独立渐变 ID） */
export function OfferFlowLogo({
  size = 34,
  className,
}: {
  size?: number;
  className?: string;
}) {
  const uid = useId().replace(/:/g, "");
  const bgId = `of-logo-bg-${uid}`;
  const glowTlId = `of-logo-glow-tl-${uid}`;
  const glowBrId = `of-logo-glow-br-${uid}`;
  const ringId = `of-logo-ring-${uid}`;
  const arrowId = `of-logo-arrow-${uid}`;

  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 512 512"
      fill="none"
      className={className}
      aria-hidden="true"
      style={{ display: "block", flexShrink: 0 }}
    >
      <defs>
        <linearGradient id={bgId} x1="36" y1="20" x2="476" y2="492" gradientUnits="userSpaceOnUse">
          <stop offset="0%" stopColor="#1A1D3A" />
          <stop offset="52%" stopColor="#1E1B4B" />
          <stop offset="100%" stopColor="#0B0F1E" />
        </linearGradient>
        <radialGradient
          id={glowTlId}
          cx="0"
          cy="0"
          r="1"
          gradientUnits="userSpaceOnUse"
          gradientTransform="translate(148 112) rotate(45) scale(370)"
        >
          <stop offset="0%" stopColor="#818CF8" stopOpacity="0.46" />
          <stop offset="52%" stopColor="#4F46E5" stopOpacity="0.16" />
          <stop offset="100%" stopColor="#4F46E5" stopOpacity="0" />
        </radialGradient>
        <radialGradient
          id={glowBrId}
          cx="0"
          cy="0"
          r="1"
          gradientUnits="userSpaceOnUse"
          gradientTransform="translate(388 396) rotate(-135) scale(240)"
        >
          <stop offset="0%" stopColor="#10B981" stopOpacity="0.24" />
          <stop offset="100%" stopColor="#10B981" stopOpacity="0" />
        </radialGradient>
        <linearGradient id={ringId} x1="128" y1="384" x2="368" y2="144" gradientUnits="userSpaceOnUse">
          <stop offset="0%" stopColor="#6366F1" />
          <stop offset="55%" stopColor="#A5B4FC" />
          <stop offset="100%" stopColor="#EEF2FF" />
        </linearGradient>
        <linearGradient id={arrowId} x1="202" y1="310" x2="366" y2="146" gradientUnits="userSpaceOnUse">
          <stop offset="0%" stopColor="#34D399" />
          <stop offset="46%" stopColor="#C7D2FE" />
          <stop offset="100%" stopColor="#FFFFFF" />
        </linearGradient>
      </defs>
      <rect x="8" y="8" width="496" height="496" rx="124" fill={`url(#${bgId})`} />
      <rect x="8" y="8" width="496" height="496" rx="124" fill={`url(#${glowTlId})`} />
      <rect x="8" y="8" width="496" height="496" rx="124" fill={`url(#${glowBrId})`} />
      <circle cx="256" cy="256" r="186" stroke="#FFFFFF" strokeOpacity="0.06" strokeWidth="2.5" />
      <path
        d="M 368 290 A 116 116 0 1 1 222 144"
        stroke={`url(#${ringId})`}
        strokeWidth="42"
        strokeLinecap="round"
      />
      <path
        d="M 222 290 L 354 158"
        stroke={`url(#${arrowId})`}
        strokeWidth="42"
        strokeLinecap="round"
      />
      <path
        d="M 284 158 H 354 V 228"
        stroke="#FFFFFF"
        strokeWidth="42"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
      <rect
        x="12"
        y="12"
        width="488"
        height="488"
        rx="120"
        stroke="#FFFFFF"
        strokeOpacity="0.16"
        strokeWidth="8"
      />
    </svg>
  );
}

/** 招聘平台专属几何徽章（BOSS / 猎聘 / 51job） */
export function PlatformBadgeIcon({
  platform,
  size = 18,
}: {
  platform: PlatformKind;
  size?: number;
}) {
  if (platform === "boss") {
    return (
      <svg width={size} height={size} viewBox="0 0 24 24" fill="none" aria-hidden="true" style={{ flexShrink: 0 }}>
        <rect x="2" y="2" width="20" height="20" rx="6" fill="#00BEBD" />
        <path
          d="M7.5 8.5C7.5 7.67 8.17 7 9 7H13.1C14.7 7 15.8 7.9 15.8 9.35C15.8 10.22 15.32 10.95 14.5 11.35C15.65 11.72 16.3 12.6 16.3 13.75C16.3 15.42 15.02 16.5 13.15 16.5H9C8.17 16.5 7.5 15.83 7.5 15V8.5Z"
          stroke="#FFFFFF"
          strokeWidth="1.9"
          strokeLinejoin="round"
        />
        <path d="M9.5 11.6H13.1" stroke="#FFFFFF" strokeWidth="1.8" strokeLinecap="round" />
      </svg>
    );
  }
  if (platform === "liepin") {
    return (
      <svg width={size} height={size} viewBox="0 0 24 24" fill="none" aria-hidden="true" style={{ flexShrink: 0 }}>
        <rect x="2" y="2" width="20" height="20" rx="6" fill="#7C3AED" />
        <path
          d="M8.5 7.5V15.2C8.5 15.92 9.08 16.5 9.8 16.5H15.8"
          stroke="#FFFFFF"
          strokeWidth="2.1"
          strokeLinecap="round"
          strokeLinejoin="round"
        />
        <circle cx="15.6" cy="9.2" r="2.1" fill="#DDD6FE" />
      </svg>
    );
  }
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" aria-hidden="true" style={{ flexShrink: 0 }}>
      <rect x="2" y="2" width="20" height="20" rx="6" fill="#F97316" />
      <path
        d="M15.5 7.8H10.2L9.6 11.4C10.3 10.95 11.1 10.75 12 10.75C13.9 10.75 15.3 11.95 15.3 13.7C15.3 15.48 13.85 16.7 11.8 16.7C10.25 16.7 9.15 16.05 8.5 15.1"
        stroke="#FFFFFF"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

/* ── 统一 1.75px 精工线性图标集 ── */

export function IconSendFlow({ size = 16, className, ...rest }: IconProps) {
  return (
    <svg {...baseProps(size, className)} {...rest}>
      <path d="M21 3L10 14" />
      <path d="M21 3L14.5 21L10 14L3 9.5L21 3Z" />
    </svg>
  );
}

export function IconKanbanFlow({ size = 16, className, ...rest }: IconProps) {
  return (
    <svg {...baseProps(size, className)} {...rest}>
      <rect x="3" y="4" width="5" height="16" rx="1.5" />
      <rect x="10" y="4" width="4.5" height="11" rx="1.5" />
      <rect x="16.5" y="4" width="4.5" height="14" rx="1.5" />
    </svg>
  );
}

export function IconSliders({ size = 16, className, ...rest }: IconProps) {
  return (
    <svg {...baseProps(size, className)} {...rest}>
      <path d="M4 7H20" />
      <path d="M4 17H20" />
      <circle cx="9" cy="7" r="2.25" fill="var(--of-surface, #fff)" />
      <circle cx="15" cy="17" r="2.25" fill="var(--of-surface, #fff)" />
    </svg>
  );
}

export function IconShieldCheck({ size = 16, className, ...rest }: IconProps) {
  return (
    <svg {...baseProps(size, className)} {...rest}>
      <path d="M12 2.75L19.25 5.8V11.6C19.25 16.35 16.15 19.95 12 21.25C7.85 19.95 4.75 16.35 4.75 11.6V5.8L12 2.75Z" />
      <path d="M9 12.1L11.1 14.2L15.4 9.8" />
    </svg>
  );
}

export function IconReplyPulse({ size = 16, className, ...rest }: IconProps) {
  return (
    <svg {...baseProps(size, className)} {...rest}>
      <path d="M20 11.5C20 15.64 16.42 19 12 19C10.65 19 9.38 18.69 8.27 18.14L4 19.5L5.22 15.86C4.45 14.61 4 13.11 4 11.5C4 7.36 7.58 4 12 4C16.42 4 20 7.36 20 11.5Z" />
      <path d="M8.75 11.5H8.76" strokeWidth="2.4" />
      <path d="M12 11.5H12.01" strokeWidth="2.4" />
      <path d="M15.25 11.5H15.26" strokeWidth="2.4" />
    </svg>
  );
}

export function IconCalendarSpark({ size = 16, className, ...rest }: IconProps) {
  return (
    <svg {...baseProps(size, className)} {...rest}>
      <rect x="3.5" y="5" width="17" height="15.5" rx="3" />
      <path d="M8 3.2V6.4" />
      <path d="M16 3.2V6.4" />
      <path d="M3.5 9.8H20.5" />
      <path d="M9.5 14.8L11.3 16.6L15 12.9" />
    </svg>
  );
}

export function IconBellAlert({ size = 16, className, ...rest }: IconProps) {
  return (
    <svg {...baseProps(size, className)} {...rest}>
      <path d="M6.5 9.8C6.5 6.76 8.96 4.3 12 4.3C15.04 4.3 17.5 6.76 17.5 9.8V13.6L19.2 16.2C19.52 16.7 19.16 17.35 18.56 17.35H5.44C4.84 17.35 4.48 16.7 4.8 16.2L6.5 13.6V9.8Z" />
      <path d="M9.8 19.6C10.3 20.32 11.1 20.75 12 20.75C12.9 20.75 13.7 20.32 14.2 19.6" />
    </svg>
  );
}

export function IconTarget({ size = 16, className, ...rest }: IconProps) {
  return (
    <svg {...baseProps(size, className)} {...rest}>
      <circle cx="12" cy="12" r="8.25" />
      <circle cx="12" cy="12" r="4.5" />
      <circle cx="12" cy="12" r="1.35" fill="currentColor" stroke="none" />
    </svg>
  );
}

export function IconResumeDoc({ size = 16, className, ...rest }: IconProps) {
  return (
    <svg {...baseProps(size, className)} {...rest}>
      <path d="M7 3.5H14.2L18.5 7.8V19C18.5 19.83 17.83 20.5 17 20.5H7C6.17 20.5 5.5 19.83 5.5 19V5C5.5 4.17 6.17 3.5 7 3.5Z" />
      <path d="M14 3.5V8H18.5" />
      <path d="M9 12.2H15" />
      <path d="M9 15.8H13.2" />
    </svg>
  );
}

export function IconBrowserCheck({ size = 16, className, ...rest }: IconProps) {
  return (
    <svg {...baseProps(size, className)} {...rest}>
      <rect x="3" y="4.5" width="18" height="15" rx="3" />
      <path d="M3 9H21" />
      <path d="M9.4 14.4L11.2 16.1L14.8 12.5" />
    </svg>
  );
}

export function IconSparkles({ size = 16, className, ...rest }: IconProps) {
  return (
    <svg {...baseProps(size, className)} {...rest}>
      <path d="M12 3L13.9 8.6L19.5 10.5L13.9 12.4L12 18L10.1 12.4L4.5 10.5L10.1 8.6L12 3Z" />
      <path d="M18.5 15.5L19.3 17.7L21.5 18.5L19.3 19.3L18.5 21.5L17.7 19.3L15.5 18.5L17.7 17.7L18.5 15.5Z" />
    </svg>
  );
}

export function IconTerminal({ size = 16, className, ...rest }: IconProps) {
  return (
    <svg {...baseProps(size, className)} {...rest}>
      <rect x="3" y="4.5" width="18" height="15" rx="3" />
      <path d="M7.5 10L10.5 12.5L7.5 15" />
      <path d="M12.5 15H16.5" />
    </svg>
  );
}

export function IconFilterSlash({ size = 16, className, ...rest }: IconProps) {
  return (
    <svg {...baseProps(size, className)} {...rest}>
      <circle cx="12" cy="12" r="8.25" />
      <path d="M6.4 6.4L17.6 17.6" />
    </svg>
  );
}

export function IconPlaySolid({ size = 16, className, ...rest }: IconProps) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="currentColor"
      aria-hidden="true"
      className={className ? `of-icon ${className}` : "of-icon"}
      {...rest}
    >
      <path d="M7.5 5.45C7.5 4.44 8.6 3.82 9.46 4.35L19.2 10.4C20 10.9 20 12.1 19.2 12.6L9.46 18.65C8.6 19.18 7.5 18.56 7.5 17.55V5.45Z" />
    </svg>
  );
}

export function IconClockRepeat({ size = 16, className, ...rest }: IconProps) {
  return (
    <svg {...baseProps(size, className)} {...rest}>
      <circle cx="12" cy="12" r="8.25" />
      <path d="M12 7.8V12.2L15 14" />
    </svg>
  );
}

export function IconSun({ size = 16, className, ...rest }: IconProps) {
  return (
    <svg {...baseProps(size, className)} {...rest}>
      <circle cx="12" cy="12" r="4" />
      <path d="M12 2.8V4.6M12 19.4V21.2M4.8 4.8L6.1 6.1M17.9 17.9L19.2 19.2M2.8 12H4.6M19.4 12H21.2M4.8 19.2L6.1 17.9M17.9 6.1L19.2 4.8" />
    </svg>
  );
}

export function IconMoon({ size = 16, className, ...rest }: IconProps) {
  return (
    <svg {...baseProps(size, className)} {...rest}>
      <path d="M19.5 14.5A7.8 7.8 0 0 1 9.5 4.5A7.9 7.9 0 1 0 19.5 14.5Z" />
    </svg>
  );
}

export function IconSystemTheme({ size = 16, className, ...rest }: IconProps) {
  return (
    <svg {...baseProps(size, className)} {...rest}>
      <rect x="3" y="4.5" width="18" height="12.5" rx="2.5" />
      <path d="M8.5 20H15.5" />
      <path d="M12 17V20" />
    </svg>
  );
}
