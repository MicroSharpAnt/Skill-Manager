import { useEffect, useRef, type InputHTMLAttributes } from "react";
export function SelectionCheckbox({
  mixed = false,
  ...props
}: InputHTMLAttributes<HTMLInputElement> & { mixed?: boolean }) {
  const ref = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (ref.current) ref.current.indeterminate = mixed;
  }, [mixed]);
  return (
    <input
      {...props}
      ref={ref}
      type="checkbox"
      aria-checked={mixed ? "mixed" : props.checked}
    />
  );
}
