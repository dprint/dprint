import * as babel from "@babel/types";

export interface BabelToken {
    start: number;
    end: number;
    value?: string;
    type?: {
        label: string;
    } | "CommentLine" | "CommentBlock";
    loc: babel.Node["loc"];
}
