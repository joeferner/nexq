import { IValidation } from "typia";

export class ConfigParseError extends Error {
  public constructor(
    public readonly configFilename: string,
    public errors: IValidation.IError[] | Error
  ) {
    super(`failed to parse config "${configFilename}"`);
    this.name = this.constructor.name;
    Error.captureStackTrace(this, this.constructor);
  }
}
