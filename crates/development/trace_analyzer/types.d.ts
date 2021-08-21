declare global {
  const rawTraceResult: TracingResult;
  const specMessage: string;
  const d3: any;
}

export interface GraphPrintNode {
  id: number;
  printNode: PrintNode;
  sources: GraphPrintNode[];
  targets: GraphPrintNode[];
  depthY: number;
}

export interface TracingResult {
  traces: Trace[];
  writerNodes: WriterNode[];
  printNodes: PrintNode[];
}

export interface Trace {
  nanos: number;
  printNodeId: number;
  writerNodeId: number | undefined;
}

export interface WriterNode {
  writerNodeId: number;
  previousNodeId: number | undefined;
  text: string;
}

export interface PrintNode {
  printNodeId: number;
  nextPrintNodeId: number | undefined;
  printItem: PrintItem;
}

export type PrintItem = InfoItem | SignalItem | StringItem | ConditionItem | RcPathItem;

export interface InfoItem {
  kind: "info";
  content: Info;
}

export interface Info {
  infoId: number;
  name: string;
}

export interface SignalItem {
  kind: "signal";
  content: Signal;
}

export type Signal =
  | "NewLine"
  | "Tab"
  | "PossibleNewLine"
  | "SpaceOrNewLine"
  | "ExpectNewLine"
  | "QueueStartIndent"
  | "StartIndent"
  | "FinishIndent"
  | "StartNewLineGroup"
  | "FinishNewLineGroup"
  | "SingleIndent"
  | "StartIgnoringIndent"
  | "FinishIgnoringIndent"
  | "StartForceNoNewLines"
  | "FinishForceNoNewLines"
  | "SpaceIfNotTrailing";

export interface StringItem {
  kind: "string";
  content: string;
}

export interface ConditionItem {
  kind: "condition";
  content: Condition;
}

export interface Condition {
  conditionId: number;
  name: string;
  isStored: boolean;
  truePath: number | undefined;
  falsePath: number | undefined;
  dependentInfos: number[] | undefined;
}

export interface RcPathItem {
  kind: "rcPath";
  content: number;
}

export type CodeViewTextSegment = TabSegment | SpaceSegment | TextSegment;

export interface TabSegment {
  kind: "tab";
  count: number;
}

export interface SpaceSegment {
  kind: "space";
  count: number;
}

export interface TextSegment {
  kind: "text";
  text: string;
}
