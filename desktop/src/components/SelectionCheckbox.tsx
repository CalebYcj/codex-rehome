import { useEffect, useRef, type InputHTMLAttributes } from "react";
export default function SelectionCheckbox({
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
      type="checkbox"
      ref={ref}
      aria-checked={mixed ? "mixed" : Boolean(props.checked)}
    />
  );
}
