import { useEffect, useState } from "react";
import { useStdout } from "ink";

export interface Dimensions {
  rows: number;
  columns: number;
}

export function useStdoutDimensions(): Dimensions {
  const { stdout } = useStdout();
  const [dimensions, setDimensions] = useState<Dimensions>({ columns: stdout.columns, rows: stdout.rows });

  useEffect(() => {
    const handler = (): void => setDimensions({ columns: stdout.columns, rows: stdout.rows });
    stdout.on("resize", handler);
    return (): void => {
      stdout.off("resize", handler);
    };
  }, [stdout]);

  return dimensions;
}
