const MAX_STABLE_FORMAT_TRIES = 5;

export function formatTextUntilStable(fileText: string, formatText: (fileText: string) => string) {
  let formattedText = formatText(fileText);
  if (formattedText === fileText) {
    return formattedText;
  }

  for (let i = 0; i < MAX_STABLE_FORMAT_TRIES; i++) {
    const nextText = formatText(formattedText);
    if (nextText === formattedText) {
      return formattedText;
    }
    formattedText = nextText;
  }

  throw new Error(
    `Formatting not stable. Bailed after ${MAX_STABLE_FORMAT_TRIES} tries. This indicates a bug in the plugin where it formats the file differently each time.`,
  );
}
