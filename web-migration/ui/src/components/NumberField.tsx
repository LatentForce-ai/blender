import { useEffect, useState } from "react";

/// A numeric input that applies every valid edit immediately and shows the engine's value
/// whenever it isn't being edited.
export function NumberField({
  label,
  value,
  onChange,
  step = 0.1,
  min,
  max,
  precision = 3,
}: {
  label: string;
  value: number;
  onChange: (value: number) => void;
  step?: number;
  min?: number;
  max?: number;
  precision?: number;
}) {
  const shown = Number(value.toFixed(precision)).toString();
  const [text, setText] = useState(shown);
  const [editing, setEditing] = useState(false);

  useEffect(() => {
    if (!editing) setText(shown);
  }, [shown, editing]);

  return (
    <label className="number-field">
      <span>{label}</span>
      <input
        type="number"
        value={text}
        step={step}
        min={min}
        max={max}
        onFocus={() => setEditing(true)}
        onBlur={() => setEditing(false)}
        onChange={(e) => {
          setText(e.target.value);
          const parsed = parseFloat(e.target.value);
          if (Number.isFinite(parsed)) onChange(parsed);
        }}
      />
    </label>
  );
}
