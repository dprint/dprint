import { throwError } from "../utils";

export interface WriterState {
    currentLineColumn: number;
    currentLineNumber: number;
    lastLineIndentLevel: number;
    indentLevel: number;
    indentText: string;
    expectNewLineNext: boolean;
    items: string[];
    indentLevelStates: number[];
    ignoreIndent: boolean;
}

export class Writer {
    private readonly singleIndentationText: string;
    private indentWidth: number;
    private fireOnNewLine?: () => void;

    private state: WriterState;

    constructor(private readonly options: { indentWidth: number; useTabs: boolean; newlineKind: "\r\n" | "\n"; }) {
        this.indentWidth = options.indentWidth;
        this.singleIndentationText = this.options.useTabs ? "\t" : " ".repeat(options.indentWidth);
        this.state = {
            currentLineColumn: 0,
            currentLineNumber: 0,
            lastLineIndentLevel: 0,
            indentLevel: 0,
            indentText: "",
            expectNewLineNext: false,
            items: [],
            indentLevelStates: [],
            ignoreIndent: false
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
            indentLevelStates: [...state.indentLevelStates],
            ignoreIndent: state.ignoreIndent
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

    private get ignoreIndent() {
        return this.state.ignoreIndent;
    }

    private set ignoreIndent(value: boolean) {
        this.state.ignoreIndent = value;
    }

    private get indentLevelStates() {
        return this.state.indentLevelStates;
    }

    private get items() {
        return this.state.items;
    }

    singleIndent() {
        this.write(this.singleIndentationText);
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

        if (this.expectNewLineNext) {
            this.expectNewLineNext = false;
            if (!startsWithNewLine) {
                this.baseWrite(this.options.newlineKind);
                this.baseWrite(text);
                return;
            }
        }

        if (this.currentLineColumn === 0 && !startsWithNewLine && this.indentLevel > 0 && !this.ignoreIndent)
            text = this.indentText + text;

        for (let i = 0; i < text.length; i++) {
            if (text[i] === "\n") {
                this.currentLineColumn = 0;
                this.currentLineNumber++;
                this.lastLineIndentLevel = this.indentLevel;
                this.fireOnNewLine!(); // expect this to be set
            }
            else {
                // update the indent level again if on the first line
                if (this.currentLineColumn === 0)
                    this.lastLineIndentLevel = this.indentLevel;

                if (text[i] === "\t")
                    this.currentLineColumn += this.indentWidth;
                else
                    this.currentLineColumn++;
            }
        }

        this.state.items.push(text);
    }

    startIndent() {
        this.indentLevelStates.push(this.indentLevel);
        this.indentLevel++;
    }

    finishIndent(): void {
        const originalIndentLevel = this.indentLevelStates.pop();
        if (originalIndentLevel == null)
            return throwError(`For some reason ${nameof(this.finishIndent)} was called without a corresponding ${nameof(this.startIndent)}.`);

        this.indentLevel = originalIndentLevel;
    }

    startIgnoringIndent() {
        this.ignoreIndent = true;
    }

    finishIgnoringIndent() {
        this.ignoreIndent = false;
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
        return this.singleIndentationText.length * this.lastLineIndentLevel;
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
