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

export type PrintItem = InfoItem | SignalItem | StringItem | ConditionItem | RcPathItem | AnchorItem | ConditionReevaluationItem;

export interface InfoItem {
  kind: "info";
  content: Info;
}

export type Info = LineNumber | ColumnNumber | IsStartOfLine | IndentLevel | LineStartColumnNumber | LineStartIndentLevel;

export interface LineNumber {
  kind: "lineNumber";
  content: InfoInner;
}

export interface ColumnNumber {
  kind: "columnNumber";
  content: InfoInner;
}

export interface IsStartOfLine {
  kind: "isStartOfLine";
  content: InfoInner;
}

export interface IndentLevel {
  kind: "indentLevel";
  content: InfoInner;
}

export interface LineStartColumnNumber {
  kind: "lineStartColumnNumber";
  content: InfoInner;
}

export interface LineStartIndentLevel {
  kind: "lineStartIndentLevel";
  content: InfoInner;
}

export interface InfoInner {
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

export interface AnchorItem {
  kind: "anchor";
  content: LineNumberAnchor;
}

export interface LineNumberAnchor {
  anchorId: number;
  name: string;
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

export interface ConditionReevaluationItem {
  kind: "conditionReevaluation";
  content: ConditionReevaluation;
}

export interface ConditionReevaluation {
  conditionId: number;
  name: string;
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
