<script lang="ts">
  let {
    value,
    step,
    min,
    max,
    label,
    format,
    disabled = false,
    onchange,
  }: {
    value: number;
    step: number;
    min: number;
    max: number;
    /** What's being changed, for the buttons' labels ("Study target"). */
    label: string;
    format: (v: number) => string;
    disabled?: boolean;
    onchange: (next: number) => void;
  } = $props();

  const round = (v: number) => Math.round(v * 100) / 100;
  const set = (v: number) => onchange(round(Math.min(max, Math.max(min, v))));
</script>

<div class="flex items-center rounded-[14px] bg-s2">
  <button
    type="button"
    aria-label={`Lower ${label}`}
    disabled={disabled || value <= min}
    onclick={() => set(value - step)}
    class="h-11 w-11 text-[22px] leading-none disabled:opacity-40">−</button
  >
  <span class="min-w-[54px] text-center text-[17px] font-semibold" aria-live="polite">{format(value)}</span>
  <button
    type="button"
    aria-label={`Raise ${label}`}
    disabled={disabled || value >= max}
    onclick={() => set(value + step)}
    class="h-11 w-11 text-[22px] leading-none disabled:opacity-40">+</button
  >
</div>
