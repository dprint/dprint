import { throwError } from "../utils";
import { Signal } from "../types";

export interface WriterState {
    currentLineColumn: number;
    currentLineNumber: number;
    lastLineIndentLevel: number;
    indentLevel: number;
    indentText: string;
    expectNewLineNext: boolean;
    items: string[];
    ignoreIndentCount: number;
}

export class Writer {
    private readonly singleIndentationText: string;
    private readonly indentWidth: number;
    private readonly newLineKind: "\r\n" | "\n";
    private readonly isTesting: boolean;
    private fireOnNewLine?: () => void;

    private state: WriterState;

    constructor(options: { indentWidth: number; useTabs: boolean; newLineKind: "\r\n" | "\n"; isTesting: boolean; }) {
        this.singleIndentationText = options.useTabs ? "\t" : " ".repeat(options.indentWidth);
        this.newLineKind = options.newLineKind;
        this.indentWidth = options.indentWidth;
        this.isTesting = options.isTesting;
        this.state = {
            currentLineColumn: 0,
            currentLineNumber: 0,
            lastLineIndentLevel: 0,
            indentLevel: 0,
            indentText: "",
            expectNewLineNext: false,
            items: [],
            ignoreIndentCount: 0
        };
    }

    onNewLine(action: () => void) {
        if (this.fireOnNewLine != null)
            throwError(`Cannot call ${nameof(this.onNewLine)} multiple times.`);
        this.fireOnNewLine = action;
    }

    getState(): Readonly<WriterState> {
        // todo: perhaps an additional method should be added that will reduce
        // the number of items in the "items" array (ex. join them and create
        // a single item array). That will need to be analyzed in some
        // performance tests though.
        return Writer.cloneState(this.state);
    }

    setState(state: Readonly<WriterState>) {
        this.state = Writer.cloneState(state);
    }

    private static cloneState(state: Readonly<WriterState>): WriterState {
        const newState: MakeRequired<WriterState> = {
            currentLineColumn: state.currentLineColumn,
            currentLineNumber: state.currentLineNumber,
            lastLineIndentLevel: state.lastLineIndentLevel,
            expectNewLineNext: state.expectNewLineNext,
            indentLevel: state.indentLevel,
            indentText: state.indentText,
            items: [...state.items],
            ignoreIndentCount: state.ignoreIndentCount
        };
        return newState;
    }

    private get currentLineColumn() {
        return this.state.currentLineColumn;
    }

    private set currentLineColumn(value: number) {
        this.state.currentLineColumn = value;
    }

    private get currentLineNumber() {
        return this.state.currentLineNumber;
    }

    private set currentLineNumber(value: number) {
        this.state.currentLineNumber = value;
    }

    private get lastLineIndentLevel() {
        return this.state.lastLineIndentLevel;
    }

    private set lastLineIndentLevel(value: number) {
        this.state.lastLineIndentLevel = value;
    }

    private get expectNewLineNext() {
        return this.state.expectNewLineNext;
    }

    private set expectNewLineNext(value: boolean) {
        this.state.expectNewLineNext = value;
    }

    private get indentLevel() {
        return this.state.indentLevel;
    }

    private set indentLevel(level: number) {
        if (this.indentLevel === level)
            return;

        this.state.indentLevel = level;
        this.state.indentText = this.singleIndentationText.repeat(level);

        // if it's on the first column, update the indent level
        // that the line started on
        if (this.currentLineColumn === 0)
            this.lastLineIndentLevel = level;
    }

    private get indentText() {
        return this.state.indentText;
    }

    private get ignoreIndentCount() {
        return this.state.ignoreIndentCount;
    }

    private set ignoreIndentCount(value: number) {
        this.state.ignoreIndentCount = value;
    }

    private get items() {
        return this.state.items;
    }

    newLine() {
        this.currentLineColumn = 0;
        this.currentLineNumber++;
        this.lastLineIndentLevel = this.indentLevel;
        this.expectNewLineNext = false;
        this.items.push(this.newLineKind);
        this.fireOnNewLine!(); // expect this to be set
    }

    singleIndent() {
        this.handleFirstColumn();
        this.currentLineColumn += this.indentWidth;
        this.items.push(this.singleIndentationText);
    }

    tab() {
        this.handleFirstColumn();
        this.currentLineColumn += this.indentWidth;
        this.items.push("\t");
    }

    space() {
        this.handleFirstColumn();
        this.currentLineColumn++;
        this.items.push(" ");
    }

    write(text: string) {
        if (this.isTesting)
            this.validateText(text);

        this.handleFirstColumn();

        this.currentLineColumn += text.length;
        this.state.items.push(text);
    }

    private handleFirstColumn() {
        if (this.expectNewLineNext) {
            this.newLine();
        }

        if (this.currentLineColumn === 0 && this.indentLevel > 0 && this.ignoreIndentCount === 0) {
            // update the indent level again since on the first column
            this.lastLineIndentLevel = this.indentLevel;

            this.currentLineColumn += this.indentWidth * this.indentLevel;
            this.items.push(this.indentText);
        }
    }

    private validateText(text: string) {
        if (text.includes("\n"))
            throwError(`Printer error: The IR generation should not write newlines. Use ${nameof.full(Signal.NewLine)} instead.`);
        if (text.includes("\t"))
            throwError(`Printer error: The IR generation should not write tabs. Use ${nameof.full(Signal.Tab)} instead.`);
    }

    startIndent() {
        this.indentLevel++;
    }

    finishIndent(): void {
        this.indentLevel--;
        if (this.indentLevel < 0)
            return throwError(`For some reason ${nameof(this.finishIndent)} was called without a corresponding ${nameof(this.startIndent)}.`);
    }

    startIgnoringIndent() {
        this.ignoreIndentCount++;
    }

    finishIgnoringIndent() {
        this.ignoreIndentCount--;
    }

    markExpectNewLine() {
        this.expectNewLineNext = true;
    }

    getLineStartIndentLevel() {
        return this.lastLineIndentLevel;
    }

    getIndentationLevel() {
        return this.indentLevel;
    }

    getLineStartColumnNumber() {
        return this.indentWidth * this.lastLineIndentLevel;
    }

    /** Gets the zero-indexed line column. */
    getLineColumn() {
        if (this.currentLineColumn === 0)
            return this.indentWidth * this.indentLevel;
        return this.currentLineColumn;
    }

    /** Gets the zero-index line number. */
    getLineNumber() {
        return this.currentLineNumber;
    }

    toString() {
        return this.items.join("");
    }
}
