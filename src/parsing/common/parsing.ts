export function parseJsLikeCommentLine(rawCommentValue: string) {
    const nonSlashIndex = getFirstNonSlashIndex();
    const startTextIndex = rawCommentValue[nonSlashIndex] === " " ? nonSlashIndex + 1 : nonSlashIndex;
    const commentValue = rawCommentValue.substring(startTextIndex).trimRight();
    const prefix = "//" + rawCommentValue.substring(0, nonSlashIndex);

    return prefix + (commentValue.length > 0 ? ` ${commentValue}` : "");

    function getFirstNonSlashIndex() {
        for (let i = 0; i < rawCommentValue.length; i++) {
            if (rawCommentValue[i] !== "/")
                return i;
        }

        return rawCommentValue.length;
    }
}
