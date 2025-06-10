import prettyBytes from "pretty-bytes";
import prettyMilliseconds from "pretty-ms";
import { NexqStyles } from "../NexqStyles.js";
import { fgColor } from "../render/color.js";

export interface Detail {
  title: string;
  value: string;
  newLineNoIndent?: boolean;
}

export function detailsToString(details: Detail[]): string {
  const maxTitleWidth = Math.max(...details.map((v) => v.title.length));
  const outputTitle = (title: string): string => {
    const spacing = Math.max(0, maxTitleWidth - title.length);
    return `${fgColor(NexqStyles.detailsTitleColor)`${title}`}${fgColor(NexqStyles.detailsTitleColonColor)`:`}${" ".repeat(spacing)}`;
  };
  let text = "";
  for (const detail of details) {
    if (text.length > 0) {
      text += "\n";
    }
    if (detail.newLineNoIndent) {
      text += outputTitle(detail.title);
      text += "\n";
      text += detail.value;
    } else {
      const detailValueText = detail.value
        .split("\n")
        .map((line, index) => {
          if (index === 0) {
            return line;
          } else {
            return `${" ".repeat(maxTitleWidth)}  ${line}`;
          }
        })
        .join("\n");
      text += `${outputTitle(detail.title)} ${fgColor(NexqStyles.detailsValueColor)`${detailValueText}`}`;
    }
  }
  return text;
}

export function attributesToString(attributes: Record<string, string>): string {
  const keys = Object.keys(attributes);
  if (keys.length === 0) {
    return "<none>";
  }
  return keys.map((key) => `${key}=${attributes[key]}`).join("\n");
}

export function bodyToString(body: string): string {
  try {
    const json = JSON.parse(body) as unknown;
    return JSON.stringify(json, null, 2);
  } catch (_err) {
    return body;
  }
}

export function formatMs(ms: number | undefined): string | undefined {
  if (ms === undefined) {
    return undefined;
  }
  return prettyMilliseconds(ms);
}

export function formatBytes(bytes: number | undefined): string | undefined {
  if (bytes === undefined) {
    return undefined;
  }
  return prettyBytes(bytes);
}
