import { TableViewOptions } from "./components/TableView.js";
import { ButtonOptions } from "./render/Button.js";
import { DialogOptions } from "./render/Dialog.js";
import { InputBoxOptions } from "./render/InputBox.js";

export class NexqStyles {
  public static readonly logoColor = "#F8AE00";
  public static readonly headerNameColor = "#F8AE00";
  public static readonly headerValueColor = "#E5E5E5";

  public static readonly helpHotkeyColor = "#3288FF";
  public static readonly helpNameColor = "#E5E5E5";

  public static readonly borderColor = "#3288FF";
  public static readonly titleColor = "#4AB9DD";
  public static readonly titleAltColor = "#FF00FA";
  public static readonly titleCountColor = "#FFFFFF";

  public static readonly tableViewHeaderTextColor = "#E5E5E5";
  public static readonly tableViewTextColor = "#92D7FF";

  public static readonly dialogBorderColor = "#3288FF";
  public static readonly dialogTitleColor = "#48B8B7";

  public static readonly buttonColor = "#6AAFAF";
  public static readonly selectedButtonColor = "#3288FF";

  public static readonly inputBoxColor = "#ffffff";
  public static readonly inputBoxBgColor = "#13211F";
  public static readonly inputBoxFocusColor = "#ffffff";
  public static readonly inputBoxFocusBgColor = "#00332D";

  public static readonly detailsTitleColor = "#4182B2";
  public static readonly detailsTitleColonColor = "#FFFFFF";
  public static readonly detailsValueColor = "#FFEFD6";

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
