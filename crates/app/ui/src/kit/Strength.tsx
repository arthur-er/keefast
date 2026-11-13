import type { StrengthScoreChoice, StrengthView } from "@/bindings";
import { cn } from "@/lib/utils";

/** The word for each score, as the prototype's generator says it. */
export const STRENGTH_LABEL: Record<StrengthScoreChoice, string> = { weak: "Weak", fair: "Fair", good: "Good", strong: "Strong" };

const SEGMENTS: Record<StrengthScoreChoice, number> = { weak: 1, fair: 2, good: 3, strong: 4 };

/** `.strength`, the 4-segment bar, with the score's word (unless `word` is off) and the
 * entropy in bits. Without a strength the bar is empty. */
export function StrengthMeter({
  strength,
  word = true,
  extra,
  className,
}: {
  strength: StrengthView | null;
  word?: boolean;
  /** Said after the bits: "20 characters". */
  extra?: string;
  className?: string;
}) {
  const label = strength ? `${STRENGTH_LABEL[strength.score]}, ${strength.bits} bits` : "No strength";
  return (
    <span className={cn("strength-line", className)} role="img" aria-label={`Strength: ${label}`}>
      <span className="strength" data-score={strength ? SEGMENTS[strength.score] : 0}>
        <i />
        <i />
        <i />
        <i />
      </span>
      {strength && word && <span className="lbl">{STRENGTH_LABEL[strength.score]}</span>}
      <span className="bits">
        {strength ? `${strength.bits} bits` : "—"}
        {extra && ` · ${extra}`}
      </span>
    </span>
  );
}
