import { isString } from "../utils.js";
import { Formatter } from "./Formatter.js";
import { JsonFormatter } from "./JsonFormatter.js";
import { PatternFormatter } from "./PatternFormatter.js";

export interface FormatterJsonConfigObj {
  name: string;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  [key: string]: any;
}

export type FormatterJsonConfig = string | FormatterJsonConfigObj;

export function formatterJsonConfigToFormatter(formatterJson: FormatterJsonConfig): Formatter {
  if (isString(formatterJson)) {
    return formatterJsonConfigToFormatter({ name: formatterJson });
  }
  if (formatterJson.name === "pattern") {
    return new PatternFormatter(formatterJson);
  }
  if (formatterJson.name === "json") {
    return new JsonFormatter(formatterJson);
  }
  throw new Error(`Unknown formatter "${formatterJson.name}"`);
}
