import { throwError } from "../utils";

interface IndentState {
    hangingIndentLevel: number | undefined;
    indentLevel: number;
}

export interface WriterState {
    currentLineColumn: number;
    currentLineNumber: number;
    lastLineIndentLevel: number;
    indentLevel: number;
    indentText: string;
    hangingIndentLevel: number | undefined;
    expectNewLineNext: boolean;
    items: string[];
    indentStates: IndentState[];
    hangingIndentStates: IndentState[];
}

export class Writer {
    private readonly singleIndentationText: string;
    private fireOnNewLine?: () => void;

    private state: WriterState;

    constructor(private readonly options: { indentSize: number; useTabs: boolean; newlineKind: "\r\n" | "\n" }) {
        this.singleIndentationText = this.options.useTabs ? "\t" : " ".repeat(options.indentSize);
        this.state = {
            currentLineColumn: 0,
            currentLineNumber: 0,
            lastLineIndentLevel: 0,
            indentLevel: 0,
            indentText: "",
            hangingIndentLevel: undefined,
            expectNewLineNext: false,
            items: [],
            indentStates: [],
            hangingIndentStates: []
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
            hangingIndentLevel: state.hangingIndentLevel,
            indentLevel: state.indentLevel,
            indentText: state.indentText,
            items: [...state.items],
            indentStates: [...state.indentStates],
            hangingIndentStates: [...state.hangingIndentStates]
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

    private get hangingIndentLevel() {
        return this.state.hangingIndentLevel;
    }

    private set hangingIndentLevel(value: number | undefined) {
        this.state.hangingIndentLevel = value;
    }

    private get indentLevel() {
        return this.state.indentLevel;
    }

    private set indentLevel(level: number) {
        if (this.indentLevel === level)
            return;

        this.state.indentLevel = level;
        this.state.indentText = this.singleIndentationText.repeat(level);
    }

    private get indentText() {
        return this.state.indentText;
    }

    private get indentStates() {
        return this.state.indentStates;
    }

    private get hangingIndentStates() {
        return this.state.hangingIndentStates;
    }

    private get items() {
        return this.state.items;
    }

    write(text: string) {
        this.validateText(text);
        this.baseWrite(text);
    }

    private validateText(text: string) {
        // todo: this check should only be done when running the tests... otherwise
        // it should be turned off for performance reasons because it will iterate
        // the entire text
        if (text === "\n" || text === "\r\n")
            return;

        if (text.includes("\n"))
            throwError("Printer error: The parser should write");
    }

    baseWrite(text: string) {
        const startsWithNewLine = text[0] === "\n" || text[0] === "\r" && text[1] === "\n";
        const isNewLine = text === "\n" || text === "\r\n";
        if (this.expectNewLineNext) {
            this.expectNewLineNext = false;
            if (!startsWithNewLine) {
                this.baseWrite(this.options.newlineKind);
                this.baseWrite(text);
                return;
            }
        }

        if (isNewLine && this.hangingIndentLevel != null) {
            this.indentLevel = this.hangingIndentLevel;
            this.hangingIndentLevel = undefined;
        }

        if (this.currentLineColumn === 0 && !startsWithNewLine && this.indentLevel > 0)
            text = this.indentText + text;

        for (let i = 0; i < text.length; i++) {
            if (text[i] === "\n") {
                this.currentLineColumn = 0;
                this.currentLineNumber++;
                this.lastLineIndentLevel = this.indentLevel;
                this.fireOnNewLine!(); // expect this to be set
            }
            else
                this.currentLineColumn++;
        }

        this.state.items.push(text);
    }

    startIndent() {
        this.indentStates.push({
            hangingIndentLevel: this.hangingIndentLevel,
            indentLevel: this.indentLevel
        });
        this.indentLevel++;
    }

    finishIndent(): void {
        const originalIndentState = this.indentStates.pop();
        if (originalIndentState == null)
            return throwError(`For some reason ${nameof(this.finishIndent)} was called without a corresponding ${nameof(this.startIndent)}.`);

        this.hangingIndentLevel = originalIndentState.hangingIndentLevel;
        this.indentLevel = originalIndentState.indentLevel;
    }

    startHangingIndent() {
        this.hangingIndentStates.push({
            hangingIndentLevel: this.hangingIndentLevel,
            indentLevel: this.indentLevel
        });
        this.hangingIndentLevel = this.indentLevel + 1;
    }

    finishHangingIndent(): void {
        const originalHangingIndentState = this.hangingIndentStates.pop();
        if (originalHangingIndentState == null)
            return throwError(`For some reason ${nameof(this.finishHangingIndent)} was called without a corresponding ${nameof(this.startHangingIndent)}.`);

        this.hangingIndentLevel = originalHangingIndentState.hangingIndentLevel;
        this.indentLevel = originalHangingIndentState.indentLevel;
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

    /** Gets the zero-indexed line column. */
    getLineColumn() {
        if (this.currentLineColumn === 0)
            return this.indentText.length;
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
