import { TableViewOptions } from "./components/TableView.js";
import { ButtonOptions } from "./render/Button.js";
import { DialogOptions } from "./render/Dialog.js";
import { InputBoxOptions } from "./render/InputBox.js";

// colors from k9s: https://github.com/derailed/k9s/blob/master/skins/stock.yaml

export class NexqStyles {
  public static readonly logoColor = "orange";
  public static readonly headerNameColor = "orange";
  public static readonly headerValueColor = "white";

  public static readonly helpHotkeyColor = "dodgerblue";
  public static readonly helpNameColor = "white";

  public static readonly borderColor = "dodgerblue";
  public static readonly titleColor = "aqua";
  public static readonly titleAltColor = "fuchsia";
  public static readonly titleCountColor = "papayawhip";

  public static readonly tableViewHeaderTextColor = "white";
  public static readonly tableViewTextColor = "lightskyblue";

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

  public static readonly tableViewStyles: Partial<TableViewOptions<unknown>> = {
    headerTextColor: NexqStyles.tableViewHeaderTextColor,
    itemTextColor: NexqStyles.tableViewTextColor,
  };

  public static readonly dialogStyles: Partial<DialogOptions> = {
    borderColor: NexqStyles.dialogBorderColor,
    titleColor: NexqStyles.dialogTitleColor,
  };

  public static readonly buttonStyles: Partial<ButtonOptions> = {
    color: NexqStyles.buttonColor,
    selectedColor: NexqStyles.selectedButtonColor,
  };

  public static readonly inputStyles: Partial<InputBoxOptions> = {
    color: NexqStyles.inputBoxColor,
    bgColor: NexqStyles.inputBoxBgColor,
    focusColor: NexqStyles.inputBoxFocusColor,
    focusBgColor: NexqStyles.inputBoxFocusBgColor,
  };
}
