import { Condition, ResolveCondition, Signal, RawString, PrintItemKind, Info } from "../types";

// internal types for printing

export type PrinterPrintItem = Signal | string | RawString | ConditionContainer | Info;

export interface PrintItemContainer {
    parent?: PrintItemContainer;
    items: PrinterPrintItem[];
}

export interface ConditionContainer {
    kind: PrintItemKind.Condition;
    /** Name for debugging purposes. */
    name: string;
    originalCondition: Condition;
    condition: ResolveCondition | Condition;
    true?: PrintItemContainer;
    false?: PrintItemContainer;
}