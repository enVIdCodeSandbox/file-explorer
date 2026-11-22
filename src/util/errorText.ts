// Is a short message (a pop-up at the bottom of the window) about something that went wrong? Pop-ups carry no
// "this is an error" flag, so the words decide. Such a message can be clicked to open the activity log.

const WORDS =
  /\b(fail(ed|s|ure)?|error|could not|couldn't|cannot|can't|can not|unable|denied|refused|not (found|allowed|supported|reachable|answering|responding|accepted|available|enough)|no longer|did not|didn't|does not|doesn't|timed out|time out|invalid|stopped|stuck|is being used|in use|locked|unreachable|problem|went wrong|isn't|wasn't)\b/i;

export function looksLikeError(message: string | null | undefined): boolean {
  if (!message) return false;
  return /^system error \d+/i.test(message) || /\(os error \d+\)/i.test(message) || WORDS.test(message);
}
