import { TableView } from "./components/TableView.js";
import { Button } from "./render/Button.js";
import { Dialog } from "./render/Dialog.js";
import { InputBox } from "./render/InputBox.js";

// colors from k9s: https://github.com/derailed/k9s/blob/master/skins/stock.yaml

export class NexqStyles {
  public static readonly logoColor = "orange";
  public static readonly headerNameColor = "orange";
  public static readonly headerValueColor = "white";

  public static readonly helpHotkeyColor = "dodgerblue";
  public static readonly helpNameColor = "gray";

  public static readonly borderColor = "dodgerblue";
  public static readonly titleColor = "aqua";
  public static readonly titleAltColor = "fuchsia";
  public static readonly titleCountColor = "papayawhip";

  public static readonly tableViewHeaderTextColor = "white";
  public static readonly tableViewTextColor = "lightskyblue";
  public static readonly tableViewSortColor = "aqua";

  public static readonly dialogBorderColor = "dodgerblue";
  public static readonly dialogTitleColor = "aqua";

  public static readonly buttonColor = "dodgerblue";
  public static readonly selectedButtonColor = "dodgerblue";

  public static readonly inputBoxColor = "white";
  public static readonly inputBoxBgColor = "darkslateblue";
  public static readonly inputBoxFocusColor = "white";
  public static readonly inputBoxFocusBgColor = "mediumblue";

  public static readonly detailsTitleColor = "steelblue";
  public static readonly detailsTitleColonColor = "white";
  public static readonly detailsValueColor = "papayawhip";

  public static applyToTableView<T>(tableView: TableView<T>): void {
    tableView.style.headerTextColor = NexqStyles.tableViewHeaderTextColor;
    tableView.style.itemTextColor = NexqStyles.tableViewTextColor;
    tableView.style.sortTextColor = NexqStyles.tableViewSortColor;
  }

  public static applyToDialog<TShowOptions, TResults>(dialog: Dialog<TShowOptions, TResults>): void {
    dialog.style.borderColor = NexqStyles.dialogBorderColor;
  }

  public static applyToButton(button: Button): void {
    button.style.color = NexqStyles.buttonColor;
    button.selectedColor = NexqStyles.selectedButtonColor;
  }

  public static applyToInputBox(inputBox: InputBox): void {
    inputBox.style.color = NexqStyles.inputBoxColor;
    inputBox.style.backgroundColor = NexqStyles.inputBoxBgColor;
    inputBox.style.focusColor = NexqStyles.inputBoxFocusColor;
    inputBox.style.focusBackgroundColor = NexqStyles.inputBoxFocusBgColor;
  }
}
