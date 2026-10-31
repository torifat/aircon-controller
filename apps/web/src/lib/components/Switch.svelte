<script lang="ts">
  let {
    checked,
    label,
    disabled = false,
    onchange,
  }: { checked: boolean; label: string; disabled?: boolean; onchange: (next: boolean) => void } =
    $props();
</script>

<!-- 44px hit area around a 46×28 track, as drawn. -->
<button
  type="button"
  role="switch"
  aria-checked={checked}
  aria-label={label}
  {disabled}
  onclick={() => onchange(!checked)}
  class="flex h-11 w-15 flex-none items-center justify-center disabled:opacity-50"
>
  <span
    class={[
      'relative block h-7 w-[46px] rounded-full transition-colors',
      checked ? 'bg-cool' : 'bg-track',
    ]}
  >
    <span
      class={[
        'absolute top-[3px] size-[22px] rounded-full bg-knob shadow-[0_1px_2px_rgb(0_0_0/0.3)] transition-[left]',
        checked ? 'left-[21px]' : 'left-[3px]',
      ]}
    ></span>
  </span>
</button>
