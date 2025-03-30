import { Box, Text, useFocus } from "ink";
import * as R from "radash";
import React, { ReactNode } from "react";
import { HEADER_HEIGHT, MAIN_BORDER_COLOR, STATUS_HEIGHT, UNSELECTED_TEXT_COLOR } from "../styles.js";
import { Input, isInputMatch } from "../utils/Input.js";
import { useStdoutDimensions } from "../utils/useStdoutDimensions.js";

const SPACE_BETWEEN_COLUMNS = 1;
const SORT_ARROW_WIDTH = 1;
const BORDERS_WIDTH = 2;
const THUMB_WIDTH = 1;

export enum SortDirection {
  Ascending = "asc",
  Descending = "desc",
}

export interface TableViewColumn<T> {
  name: string;
  sortKeyboardShortcut?: string;
  sortRows: (rows: T[], direction: SortDirection) => T[];
  valueFn: (row: T) => string | number;
}

export interface TableViewState<T> {
  offset: number;
  selectedRowId: string | null;
  sortColumn: TableViewColumn<T>;
  sortColumnDirection: SortDirection;
}

export interface TableViewProps<T> {
  id: string;
  columns: TableViewColumn<T>[];
  input: Input | null;
  rows: (T & { id: string })[];
  stateChanged: (state: TableViewState<T>) => unknown;
  state: TableViewState<T>;
}

export interface _TableViewProps<T> extends TableViewProps<T> {
  isFocused: boolean;
  displayRows: number;
  displayColumns: number;
  input: Input | null;
}

class _TableView<T> extends React.Component<_TableViewProps<T>> {
  public override componentDidUpdate(prevProps: Readonly<_TableViewProps<T>>, _prevState: Readonly<unknown>): void {
    if (this.props.isFocused && this.props.input && this.props.input?.t !== prevProps.input?.t) {
      this.processInput(this.props.input);
    }

    if (this.props.state !== prevProps.state) {
      if (this.props.state.selectedRowId) {
        const { selectedRowId, offset: stateOffset } = this.props.state;

        const selectedIndex = this.props.rows.findIndex((r) => r.id === selectedRowId);
        let offset: number;
        if (selectedIndex >= stateOffset + (this.props.displayRows - 3)) {
          offset = selectedIndex - (this.props.displayRows - 4);
        } else if (selectedIndex < stateOffset) {
          offset = selectedIndex;
        } else {
          offset = stateOffset;
        }
        if (this.props.state.offset !== offset) {
          this.props.stateChanged({
            ...this.props.state,
            offset,
          });
        }
      }
    }
  }

  private processInput(input: Input): void {
    const { key, text } = input;

    if (key.upArrow || text === "OA") {
      this.setSelectedIndexHelper((value) => value - 1);
    } else if (key.downArrow || text === "OB") {
      this.setSelectedIndexHelper((value) => value + 1);
    } else if (key.pageDown) {
      this.setSelectedIndexHelper((value) => value + this.props.displayRows);
    } else if (key.pageUp) {
      this.setSelectedIndexHelper((value) => value - this.props.displayRows);
    }

    for (const column of this.props.columns) {
      if (column.sortKeyboardShortcut && isInputMatch(input, column.sortKeyboardShortcut)) {
        const isSameColumn = this.props.state.sortColumn?.name === column.name;
        this.props.stateChanged({
          ...this.props.state,
          sortColumn: column,
          sortColumnDirection:
            isSameColumn && this.props.state.sortColumnDirection === SortDirection.Ascending
              ? SortDirection.Descending
              : SortDirection.Ascending,
        });
        break;
      }
    }
  }

  public override render(): ReactNode {
    const { rows, columns, displayColumns, displayRows } = this.props;
    const { offset, sortColumn, sortColumnDirection, selectedRowId } = this.props.state;

    const selectedIndex = selectedRowId ? rows.findIndex((r) => r.id === selectedRowId) : -1;

    const columnWidths = columns.map((column) => {
      return (
        Math.max(...rows.map((row) => `${column.valueFn(row)}`.length), column.name.length + SORT_ARROW_WIDTH) +
        SPACE_BETWEEN_COLUMNS
      );
    });

    return (
      <Box
        flexDirection="column"
        borderStyle="single"
        borderColor={MAIN_BORDER_COLOR}
        width={displayColumns}
        height={displayRows}
      >
        <TableViewHeader
          columns={columns}
          sortColumn={sortColumn}
          sortColumnDirection={sortColumnDirection}
          columnWidths={columnWidths}
        />
        <Box flexDirection="row" justifyContent="space-between">
          <Box flexDirection="column">
            {rows.slice(offset, offset + displayRows - 3).map((row: T, rowIndex) => {
              const selected = selectedIndex === rowIndex + offset;
              return (
                <Box key={rowIndex}>
                  <TableViewRow<T>
                    columns={columns}
                    columnWidths={columnWidths}
                    viewportColumns={displayColumns}
                    row={row}
                    selected={selected}
                  />
                </Box>
              );
            })}
          </Box>
          <TableViewThumb selectedIndex={selectedIndex} rowCount={rows.length} displayRows={displayRows - 3} />
        </Box>
      </Box>
    );
  }

  private setSelectedIndexHelper(changer: (value: number) => number): void {
    const { selectedRowId } = this.props.state;
    const prevSelectedIndex = selectedRowId ? this.props.rows.findIndex((r) => r.id === selectedRowId) : 0;
    const next = changer(prevSelectedIndex);
    const selectedIndex = Math.max(0, Math.min(this.props.rows.length - 1, next));
    this.props.stateChanged({
      ...this.props.state,
      selectedRowId: this.props.rows[selectedIndex]?.id ?? null,
    });
  }
}

interface TableViewHeaderProps<T> {
  columns: TableViewColumn<T>[];
  sortColumn: TableViewColumn<T> | null;
  sortColumnDirection: SortDirection;
  columnWidths: number[];
}

function TableViewHeader<T>(props: TableViewHeaderProps<T>): React.ReactNode {
  const { columns, columnWidths, sortColumn, sortColumnDirection } = props;

  return (
    <Box>
      {columns.map((column, columnIndex) => {
        return (
          <>
            <Text key={columnIndex}>{column.name}</Text>
            {column.name === sortColumn?.name ? <SortArrow direction={sortColumnDirection} /> : <Text> </Text>}
            <Text>
              {" ".repeat(
                Math.max(0, columnWidths[columnIndex] - column.name.length - SORT_ARROW_WIDTH) + SPACE_BETWEEN_COLUMNS
              )}
            </Text>
          </>
        );
      })}
    </Box>
  );
}

function SortArrow(props: { direction?: SortDirection }): React.ReactNode {
  const { direction } = props;

  if (direction === undefined) {
    return <></>;
  } else if (direction === SortDirection.Ascending) {
    return <Text>↑</Text>;
  } else if (direction === SortDirection.Descending) {
    return <Text>↓</Text>;
  }
}

function TableViewRow<T>(props: {
  columns: TableViewColumn<T>[];
  columnWidths: number[];
  viewportColumns: number;
  row: T;
  selected: boolean;
}): React.ReactNode {
  const { columns, columnWidths, row, selected, viewportColumns } = props;

  const remainingSpace = Math.max(
    0,
    viewportColumns - columnWidths.reduce((p, v) => p + v + SPACE_BETWEEN_COLUMNS, 0) - BORDERS_WIDTH - THUMB_WIDTH
  );
  return (
    <Box>
      {columns.map((column, columnIndex) => {
        const value = column.valueFn(row);
        const valueStr = `${value}`;
        const alignLeft = R.isString(value);
        const paddingWidth = Math.max(
          0,
          columnWidths[columnIndex] - valueStr.length - SORT_ARROW_WIDTH - SPACE_BETWEEN_COLUMNS
        );
        const padding = " ".repeat(paddingWidth);
        const paddingLeft = alignLeft ? "" : padding;
        const paddingRight = alignLeft ? padding : "";
        const text = `${paddingLeft}${valueStr}${paddingRight}   `;
        return (
          <Text key={columnIndex} color={UNSELECTED_TEXT_COLOR} inverse={selected}>
            {text}
          </Text>
        );
      })}
      <Text color={UNSELECTED_TEXT_COLOR} inverse={selected}>
        {" ".repeat(remainingSpace)}
      </Text>
    </Box>
  );
}

function TableViewThumb(props: { selectedIndex: number; rowCount: number; displayRows: number }): React.ReactNode {
  const { selectedIndex, rowCount, displayRows } = props;

  const thumbPosition = Math.floor((selectedIndex / rowCount) * displayRows);
  if (rowCount > displayRows) {
    return (
      <Box width={1} marginTop={thumbPosition}>
        <Text>|</Text>
      </Box>
    );
  } else {
    return <></>;
  }
}

export function TableView<T>(props: TableViewProps<T>): ReactNode {
  const { isFocused } = useFocus({ id: props.id });
  const { rows, columns } = useStdoutDimensions();

  return (
    <_TableView
      {...props}
      isFocused={isFocused}
      displayRows={rows - HEADER_HEIGHT - STATUS_HEIGHT}
      displayColumns={columns}
    />
  );
}
