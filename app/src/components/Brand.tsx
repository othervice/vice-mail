import { useId } from "react";

/*
 * The brand artwork of docs/brand (decision 0038, docs/brand/README.md), inlined as SVG so the
 * app loads nothing and needs no image files: icon.svg and the wordmark. Generated from
 * those files (attributes in JSX spelling); when the artwork changes, regenerate this file from
 * docs/brand rather than editing paths here. Each instance prefixes the ids of its gradients and
 * filters, so several copies on one page never refer to each other's definitions. Only SVG
 * attributes, no style elements (CSP). The wordmark draws "Vice" in `currentColor`: the brand's
 * ink on light, white on dark (`text-brand-ink`); everything orange keeps its own colours.
 */

function useBrandIdPrefix(): string {
  return `brand-${useId().replace(/[^a-zA-Z0-9_-]/g, "")}`;
}

/** The app icon (rounded square, mark, glow), for the About page. Decorative: the name stands next to it. */
export function AppIcon({ className }: { className?: string }) {
  const p = useBrandIdPrefix();
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      viewBox="0 0 1024 1024"
      className={className}
      aria-hidden
      focusable="false"
    >
      <defs>
        <linearGradient id={`${p}-rim-edge`} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="#ffd43a" />
          <stop offset="0.55" stopColor="#ffb11a" />
          <stop offset="1" stopColor="#ff8a0a" />
        </linearGradient>
        <linearGradient id={`${p}-rim`} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="#ff8c00" />
          <stop offset="0.55" stopColor="#ff6c00" />
          <stop offset="1" stopColor="#e84a00" />
        </linearGradient>
        <radialGradient id={`${p}-night`} cx="0.5" cy="0.36" r="0.7">
          <stop offset="0" stopColor="#1b1213" />
          <stop offset="1" stopColor="#0e0a0b" />
        </radialGradient>
        <clipPath id={`${p}-inner`}>
          <path d="M373.9 118H650.1C723.79 118 760.63 118 800.29 130.54C843.59 146.3 877.7 180.41 893.46 223.71C906 263.37 906 300.21 906 373.9V650.1C906 723.79 906 760.63 893.46 800.29C877.7 843.59 843.59 877.7 800.29 893.46C760.63 906 723.79 906 650.1 906H373.9C300.21 906 263.37 906 223.71 893.46C180.41 877.7 146.3 843.59 130.54 800.29C118 760.63 118 723.79 118 650.1V373.9C118 300.21 118 263.37 130.54 223.71C146.3 180.41 180.41 146.3 223.71 130.54C263.37 118 300.21 118 373.9 118Z" />
        </clipPath>
        <linearGradient id={`${p}-v-left`} gradientUnits="userSpaceOnUse" x1="0" y1="300" x2="0" y2="690">
          <stop offset="0" stopColor="#ffc21f" />
          <stop offset="0.3" stopColor="#ff9c0e" />
          <stop offset="0.62" stopColor="#ff6c00" />
          <stop offset="1" stopColor="#f04e00" />
        </linearGradient>
        <linearGradient id={`${p}-v-right`} gradientUnits="userSpaceOnUse" x1="0" y1="300" x2="0" y2="690">
          <stop offset="0" stopColor="#ffcb2b" />
          <stop offset="0.32" stopColor="#ffa816" />
          <stop offset="0.66" stopColor="#ff7700" />
          <stop offset="1" stopColor="#f45600" />
        </linearGradient>
        <linearGradient id={`${p}-fold`} gradientUnits="userSpaceOnUse" x1="0" y1="300" x2="0" y2="517">
          <stop offset="0" stopColor="#ffe27a" stopOpacity="0.95" />
          <stop offset="1" stopColor="#ffb020" stopOpacity="0.15" />
        </linearGradient>
        <linearGradient id={`${p}-outline`} gradientUnits="userSpaceOnUse" x1="0" y1="452" x2="0" y2="778">
          <stop offset="0" stopColor="#ffb31c" />
          <stop offset="0.45" stopColor="#ff8a06" />
          <stop offset="1" stopColor="#f45a00" />
        </linearGradient>
        <linearGradient id={`${p}-back`} gradientUnits="userSpaceOnUse" x1="0" y1="372" x2="0" y2="778">
          <stop offset="0" stopColor="#0c0c0f" />
          <stop offset="1" stopColor="#1d1c22" />
        </linearGradient>
        <linearGradient id={`${p}-bottom-flap`} gradientUnits="userSpaceOnUse" x1="0" y1="589" x2="0" y2="778">
          <stop offset="0" stopColor="#302f36" />
          <stop offset="1" stopColor="#121115" />
        </linearGradient>
        <linearGradient id={`${p}-top-flap`} gradientUnits="userSpaceOnUse" x1="192" y1="0" x2="832" y2="0">
          <stop offset="0" stopColor="#6a6b77" />
          <stop offset="0.2" stopColor="#34343c" />
          <stop offset="0.5" stopColor="#222127" />
          <stop offset="0.8" stopColor="#34343c" />
          <stop offset="1" stopColor="#6a6b77" />
        </linearGradient>
        <linearGradient id={`${p}-speed`} x1="0" y1="0" x2="1" y2="0">
          <stop offset="0" stopColor="#ff9a0a" />
          <stop offset="1" stopColor="#ff7a00" stopOpacity="0" />
        </linearGradient>
        <filter id={`${p}-blur6`} x="-25%" y="-25%" width="150%" height="150%">
          <feGaussianBlur stdDeviation="6" />
        </filter>
        <filter id={`${p}-blur9`} x="-25%" y="-25%" width="150%" height="150%">
          <feGaussianBlur stdDeviation="9" />
        </filter>
        <filter id={`${p}-blur10`} x="-25%" y="-25%" width="150%" height="150%">
          <feGaussianBlur stdDeviation="10" />
        </filter>
        <filter id={`${p}-blur14`} x="-25%" y="-25%" width="150%" height="150%">
          <feGaussianBlur stdDeviation="14" />
        </filter>
        <filter id={`${p}-blur18`} x="-25%" y="-25%" width="150%" height="150%">
          <feGaussianBlur stdDeviation="18" />
        </filter>
      </defs>
      <path d="M383.41 100H640.59C722.19 100 763 100 806.92 113.89C854.88 131.34 892.66 169.12 910.11 217.08C924 261 924 301.81 924 383.41V640.59C924 722.19 924 763 910.11 806.92C892.66 854.88 854.88 892.66 806.92 910.11C763 924 722.19 924 640.59 924H383.41C301.81 924 261 924 217.08 910.11C169.12 892.66 131.34 854.88 113.89 806.92C100 763 100 722.19 100 640.59V383.41C100 301.81 100 261 113.89 217.08C131.34 169.12 169.12 131.34 217.08 113.89C261 100 301.81 100 383.41 100Z" transform="translate(0 12)" opacity="0.45" filter={`url(#${p}-blur14)`} />
      <path d="M383.41 100H640.59C722.19 100 763 100 806.92 113.89C854.88 131.34 892.66 169.12 910.11 217.08C924 261 924 301.81 924 383.41V640.59C924 722.19 924 763 910.11 806.92C892.66 854.88 854.88 892.66 806.92 910.11C763 924 722.19 924 640.59 924H383.41C301.81 924 261 924 217.08 910.11C169.12 892.66 131.34 854.88 113.89 806.92C100 763 100 722.19 100 640.59V383.41C100 301.81 100 261 113.89 217.08C131.34 169.12 169.12 131.34 217.08 113.89C261 100 301.81 100 383.41 100Z" fill={`url(#${p}-rim-edge)`} />
      <path d="M380.24 106H643.76C722.72 106 762.21 106 804.71 119.44C851.12 136.33 887.67 172.88 904.56 219.29C918 261.79 918 301.28 918 380.24V643.76C918 722.72 918 762.21 904.56 804.71C887.67 851.12 851.12 887.67 804.71 904.56C762.21 918 722.72 918 643.76 918H380.24C301.28 918 261.79 918 219.29 904.56C172.88 887.67 136.33 851.12 119.44 804.71C106 762.21 106 722.72 106 643.76V380.24C106 301.28 106 261.79 119.44 219.29C136.33 172.88 172.88 136.33 219.29 119.44C261.79 106 301.28 106 380.24 106Z" fill={`url(#${p}-rim)`} />
      <path d="M373.9 118H650.1C723.79 118 760.63 118 800.29 130.54C843.59 146.3 877.7 180.41 893.46 223.71C906 263.37 906 300.21 906 373.9V650.1C906 723.79 906 760.63 893.46 800.29C877.7 843.59 843.59 877.7 800.29 893.46C760.63 906 723.79 906 650.1 906H373.9C300.21 906 263.37 906 223.71 893.46C180.41 877.7 146.3 843.59 130.54 800.29C118 760.63 118 723.79 118 650.1V373.9C118 300.21 118 263.37 130.54 223.71C146.3 180.41 180.41 146.3 223.71 130.54C263.37 118 300.21 118 373.9 118Z" fill={`url(#${p}-night)`} />
      <g clipPath={`url(#${p}-inner)`}>
        <path d="M373.9 118H650.1C723.79 118 760.63 118 800.29 130.54C843.59 146.3 877.7 180.41 893.46 223.71C906 263.37 906 300.21 906 373.9V650.1C906 723.79 906 760.63 893.46 800.29C877.7 843.59 843.59 877.7 800.29 893.46C760.63 906 723.79 906 650.1 906H373.9C300.21 906 263.37 906 223.71 893.46C180.41 877.7 146.3 843.59 130.54 800.29C118 760.63 118 723.79 118 650.1V373.9C118 300.21 118 263.37 130.54 223.71C146.3 180.41 180.41 146.3 223.71 130.54C263.37 118 300.21 118 373.9 118Z" fill="none" stroke="#ff5a00" strokeWidth="26" opacity="0.6" filter={`url(#${p}-blur10)`} />
        <path d="M373.9 118H650.1C723.79 118 760.63 118 800.29 130.54C843.59 146.3 877.7 180.41 893.46 223.71C906 263.37 906 300.21 906 373.9V650.1C906 723.79 906 760.63 893.46 800.29C877.7 843.59 843.59 877.7 800.29 893.46C760.63 906 723.79 906 650.1 906H373.9C300.21 906 263.37 906 223.71 893.46C180.41 877.7 146.3 843.59 130.54 800.29C118 760.63 118 723.79 118 650.1V373.9C118 300.21 118 263.37 130.54 223.71C146.3 180.41 180.41 146.3 223.71 130.54C263.37 118 300.21 118 373.9 118Z" fill="none" stroke="#ff7a10" strokeWidth="10" opacity="0.7" filter={`url(#${p}-blur6)`} />
      </g>
      <path d="M205.0 452V712A53.0 53.0 0 0 0 258 765.0H766A53.0 53.0 0 0 0 819.0 712V452" fill="none" stroke="#ff6a00" strokeWidth="36" opacity="0.5" filter={`url(#${p}-blur10)`} />
      <path d="M194 412A38 38 0 0 1 232 374H792A38 38 0 0 1 830 412V712A64 64 0 0 1 766 776H258A64 64 0 0 1 194 712Z" fill={`url(#${p}-back)`} />
      <polygon points="229.6,740.4 396,589 628,589 794.4,740.4 766,770 258,770" fill={`url(#${p}-bottom-flap)`} />
      <path d="M229.6 740.4L396 589M794.4 740.4L628 589M194 417L396 589M830 417L628 589" fill="none" stroke="#ff7a00" strokeWidth="12" opacity="0.55" filter={`url(#${p}-blur6)`} />
      <path d="M205.0 452V712A53.0 53.0 0 0 0 258 765.0H766A53.0 53.0 0 0 0 819.0 712V452" fill="none" stroke={`url(#${p}-outline)`} strokeWidth="26" strokeLinecap="round" />
      <path d="M192 416V412A40 40 0 0 1 232 372H792A40 40 0 0 1 832 412V416L628 589L396 589Z" fill={`url(#${p}-top-flap)`} />
      <path d="M229.6 740.4L396 589M794.4 740.4L628 589M194 417L396 589M830 417L628 589" fill="none" stroke="#ffa324" strokeWidth="3.5" strokeLinecap="round" />
      <path d="M193.5 410V412A38.5 38.5 0 0 1 232 373.5H278M830.5 410V412A38.5 38.5 0 0 0 792 373.5H746" fill="none" stroke="#d5d6de" strokeWidth="3" strokeLinecap="round" opacity="0.75" />
      <polygon points="210,300 355,300 512,517 669,300 814,300 512,690" transform="translate(0 22)" opacity="0.8" filter={`url(#${p}-blur18)`} />
      <polygon points="210,300 355,300 512,517 669,300 814,300 512,690" fill="#ff6400" opacity="0.9" filter={`url(#${p}-blur9)`} />
      <polygon points="210,300 355,300 512,517 512,690" fill={`url(#${p}-v-left)`} />
      <polygon points="669,300 814,300 512,690 512,517" fill={`url(#${p}-v-right)`} />
      <polygon points="512,517 669,300 703,300 520,539" fill={`url(#${p}-fold)`} />
      <path d="M210 300L512 690L814 300M355 300L512 517L669 300" fill="none" stroke="#ffd862" strokeWidth="2.5" strokeLinejoin="round" opacity="0.85" />
    </svg>
  );
}

/**
 * Mark and name as outlines; named for screen readers by the caller (the product name), or
 * decorative when the caller writes the name as text beside it.
 * `lettering` shows only the name (the same outlines, the mark cropped away), for places where
 * the app icon stands right next to it and the mark would appear twice.
 */
export function Wordmark({ className, lettering = false, ...props }: { className?: string; lettering?: boolean; "aria-label"?: string }) {
  const p = useBrandIdPrefix();
  // Without a label the artwork is decorative (the name stands next to it as text).
  const label = props["aria-label"];
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      viewBox={lettering ? "306 38 606 117" : "0 -0.5 910.48 206"}
      className={className}
      role={label ? "img" : undefined}
      aria-label={label}
      aria-hidden={label ? undefined : true}
      focusable="false"
    >
      <defs>
        <linearGradient id={`${p}-m-v-left`} gradientUnits="userSpaceOnUse" x1="0" y1="300" x2="0" y2="690">
          <stop offset="0" stopColor="#ffc21f" />
          <stop offset="0.3" stopColor="#ff9c0e" />
          <stop offset="0.62" stopColor="#ff6c00" />
          <stop offset="1" stopColor="#f04e00" />
        </linearGradient>
        <linearGradient id={`${p}-m-v-right`} gradientUnits="userSpaceOnUse" x1="0" y1="300" x2="0" y2="690">
          <stop offset="0" stopColor="#ffcb2b" />
          <stop offset="0.32" stopColor="#ffa816" />
          <stop offset="0.66" stopColor="#ff7700" />
          <stop offset="1" stopColor="#f45600" />
        </linearGradient>
        <linearGradient id={`${p}-m-fold`} gradientUnits="userSpaceOnUse" x1="0" y1="300" x2="0" y2="517">
          <stop offset="0" stopColor="#ffe27a" stopOpacity="0.95" />
          <stop offset="1" stopColor="#ffb020" stopOpacity="0.15" />
        </linearGradient>
        <linearGradient id={`${p}-m-outline`} gradientUnits="userSpaceOnUse" x1="0" y1="452" x2="0" y2="778">
          <stop offset="0" stopColor="#ffb31c" />
          <stop offset="0.45" stopColor="#ff8a06" />
          <stop offset="1" stopColor="#f45a00" />
        </linearGradient>
        <linearGradient id={`${p}-m-back`} gradientUnits="userSpaceOnUse" x1="0" y1="372" x2="0" y2="778">
          <stop offset="0" stopColor="#0c0c0f" />
          <stop offset="1" stopColor="#1d1c22" />
        </linearGradient>
        <linearGradient id={`${p}-m-bottom-flap`} gradientUnits="userSpaceOnUse" x1="0" y1="589" x2="0" y2="778">
          <stop offset="0" stopColor="#302f36" />
          <stop offset="1" stopColor="#121115" />
        </linearGradient>
        <linearGradient id={`${p}-m-top-flap`} gradientUnits="userSpaceOnUse" x1="192" y1="0" x2="832" y2="0">
          <stop offset="0" stopColor="#6a6b77" />
          <stop offset="0.2" stopColor="#34343c" />
          <stop offset="0.5" stopColor="#222127" />
          <stop offset="0.8" stopColor="#34343c" />
          <stop offset="1" stopColor="#6a6b77" />
        </linearGradient>
        <linearGradient id={`${p}-m-speed`} x1="0" y1="0" x2="1" y2="0">
          <stop offset="0" stopColor="#ff9a0a" />
          <stop offset="1" stopColor="#ff7a00" stopOpacity="0" />
        </linearGradient>
        <linearGradient id={`${p}-mail`} gradientUnits="userSpaceOnUse" x1="0" y1="0" x2="0" y2="100">
          <stop offset="0" stopColor="#ff8c00" />
          <stop offset="1" stopColor="#f45800" />
        </linearGradient>
      </defs>
      <g transform="scale(0.43) translate(-192 -300)">
        <path d="M194 412A38 38 0 0 1 232 374H792A38 38 0 0 1 830 412V712A64 64 0 0 1 766 776H258A64 64 0 0 1 194 712Z" fill={`url(#${p}-m-back)`} />
        <polygon points="229.6,740.4 396,589 628,589 794.4,740.4 766,770 258,770" fill={`url(#${p}-m-bottom-flap)`} />
        <path d="M205.0 452V712A53.0 53.0 0 0 0 258 765.0H766A53.0 53.0 0 0 0 819.0 712V452" fill="none" stroke={`url(#${p}-m-outline)`} strokeWidth="26" strokeLinecap="round" />
        <path d="M192 416V412A40 40 0 0 1 232 372H792A40 40 0 0 1 832 412V416L628 589L396 589Z" fill={`url(#${p}-m-top-flap)`} />
        <path d="M229.6 740.4L396 589M794.4 740.4L628 589M194 417L396 589M830 417L628 589" fill="none" stroke="#ffa324" strokeWidth="3.5" strokeLinecap="round" />
        <path d="M193.5 410V412A38.5 38.5 0 0 1 232 373.5H278M830.5 410V412A38.5 38.5 0 0 0 792 373.5H746" fill="none" stroke="#d5d6de" strokeWidth="3" strokeLinecap="round" opacity="0.75" />
        <polygon points="210,300 355,300 512,517 512,690" fill={`url(#${p}-m-v-left)`} />
        <polygon points="669,300 814,300 512,690 512,517" fill={`url(#${p}-m-v-right)`} />
        <polygon points="512,517 669,300 703,300 520,539" fill={`url(#${p}-m-fold)`} />
        <path d="M210 300L512 690L814 300M355 300L512 517L669 300" fill="none" stroke="#ffd862" strokeWidth="2.5" strokeLinejoin="round" opacity="0.85" />
        <rect x="827.61" y="307" width="182.39" height="18" rx="9" fill={`url(#${p}-m-speed)`} />
        <rect x="799.73" y="343" width="175.27" height="18" rx="9" fill={`url(#${p}-m-speed)`} />
        <rect x="771.86" y="379" width="163.14" height="18" rx="9" fill={`url(#${p}-m-speed)`} />
      </g>
      <g transform="translate(308.48 52.5)" fill="currentColor">
        <path transform="translate(0 0)" d="M0 0H24.4L57 67.91L89.6 0H114L66 100H48Z" fillRule="evenodd" />
        <path transform="translate(124 0)" d="M0 20H22V100H0ZM0 -9A2 2 0 0 1 2 -11H20A2 2 0 0 1 22 -9V11H0Z" fillRule="evenodd" />
        <path transform="translate(156 0)" d="M70 20H24A24 24 0 0 0 0 44V76A24 24 0 0 0 24 100H70V81H28A6 6 0 0 1 22 75V45A6 6 0 0 1 28 39H70Z" fillRule="evenodd" />
        <path transform="translate(236 0)" d="M50 20H24A24 24 0 0 0 0 44V76A24 24 0 0 0 24 100H74V81H28A6 6 0 0 1 22 75V67H74V44A24 24 0 0 0 50 20ZM28 39H46A6 6 0 0 1 52 45V51H22V45A6 6 0 0 1 28 39Z" fillRule="evenodd" />
      </g>
      <g transform="translate(648.48 52.5)" fill={`url(#${p}-mail)`}>
        <path transform="translate(0 0)" d="M0 100V0H25L60 42L95 0H120V100H98V29.2L60 74.8L22 29.2V100Z" fillRule="evenodd" />
        <path transform="translate(130 0)" d="M5 20H44A24 24 0 0 1 68 44V100H18A18 18 0 0 1 0 82V73A18 18 0 0 1 18 55H46V45A6 6 0 0 0 40 39H5ZM28 71H46V81H28A6 6 0 0 1 22 75V77A6 6 0 0 1 28 71Z" fillRule="evenodd" />
        <path transform="translate(208 0)" d="M0 20H22V100H0ZM0 -9A2 2 0 0 1 2 -11H20A2 2 0 0 1 22 -9V11H0Z" fillRule="evenodd" />
        <path transform="translate(240 0)" d="M0 0H22V100H0Z" fillRule="evenodd" />
      </g>
    </svg>
  );
}
