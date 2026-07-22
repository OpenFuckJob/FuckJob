import { commonCss } from "./common";
import { defaultCss } from "./default";
import { blueCss } from "./blue";
import { orangeCss } from "./orange";
import { blueWeightCss } from "./blue-weight";
import { vertical1Css } from "./vertical-1";

const themeCss: Record<string, string> = {
  default: commonCss + defaultCss,
  blue: commonCss + blueCss,
  pingmian: commonCss + orangeCss,
  "blue-weight": commonCss + blueWeightCss,
  "vertical-1": commonCss + vertical1Css,
};

export function getThemeCss(theme: string): string {
  return themeCss[theme] || themeCss.default;
}
