import { InvalidArgumentError } from "./error/InvalidArgumentError.js";

export interface ExternalScalerMetadata {
  queueName: string;
  threshold: number;
  activationThreshold: number;
}

export function parseExternalScalerMetadata(metadata: Record<string, string>): ExternalScalerMetadata {
  if (!metadata.queueName) {
    throw new InvalidArgumentError('"queueName" is required');
  }
  const queueName = metadata.queueName;

  if (!metadata.threshold) {
    throw new InvalidArgumentError('"threshold" is required');
  }
  const threshold = Number(metadata.threshold);
  if (isNaN(threshold)) {
    throw new InvalidArgumentError('"threshold" must be a number');
  }

  const activationThreshold = parseFloat(metadata.activationThreshold ?? "0");
  return {
    queueName,
    threshold,
    activationThreshold,
  };
}
