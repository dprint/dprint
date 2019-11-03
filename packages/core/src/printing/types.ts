import { Condition, ConditionResolver, Signal, PrintItemKind, Info } from "@dprint/types";

// internal types for printing

export type PrinterPrintItem = Signal | string | ConditionContainer | Info;

export interface PrintItemContainer {
    parent?: PrintItemContainer;
    items: PrinterPrintItem[];
}

export interface ConditionContainer {
    kind: PrintItemKind.Condition;
    /** Name for debugging purposes. */
    name: string;
    originalCondition: Condition;
    condition: ConditionResolver | Condition;
    true?: PrintItemContainer;
    false?: PrintItemContainer;
}
