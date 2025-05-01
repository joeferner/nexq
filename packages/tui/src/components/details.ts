import { hex } from "ansis";
import { NexqStyles } from "../NexqStyles.js";

export interface Detail {
  title: string;
  value: string;
  newLineNoIndent?: boolean;
}

export function detailsToString(details: Detail[]): string {
  const maxTitleWidth = Math.max(...details.map((v) => v.title.length));
  const outputTitle = (title: string): string => {
    const spacing = Math.max(0, maxTitleWidth - title.length);
    return `${hex(NexqStyles.detailsTitleColor)`${title}`}${hex(NexqStyles.detailsTitleColonColor)`:`}${" ".repeat(spacing)}`;
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
      text += `${outputTitle(detail.title)} ${hex(NexqStyles.detailsValueColor)`${detailValueText}`}`;
    }
  }
  return text;
}
