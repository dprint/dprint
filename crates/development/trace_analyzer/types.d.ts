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
  /** Why printing the node made the printer go back to an earlier point. It continues from the next trace's print node. */
  jump: Jump | undefined;
}

export type Jump = NewLineJump | InfoJump | ConditionJump | ConditionReevaluationJump;

/** The line was too long, so the printer went back to the last possible newline to write a newline there. */
export interface NewLineJump {
  kind: "newLine";
}

/** An info was printed after a condition used it before it was resolved. */
export interface InfoJump {
  kind: "info";
  content: {
    info: Info;
    value: number | boolean;
  };
}

/** A condition was printed after another condition used its value before it was resolved. */
export interface ConditionJump {
  kind: "condition";
  content: ConditionJumpContent;
}

/** A condition was re-evaluated and its value changed. */
export interface ConditionReevaluationJump {
  kind: "conditionReevaluation";
  content: ConditionJumpContent;
}

export interface ConditionJumpContent {
  conditionId: number;
  name: string;
  value: boolean | undefined;
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
  | "StartResettingIndent"
  | "FinishResettingIndent"
  | { StartAlign: number }
  | "FinishAlign"
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
