// FollowSwitch: Interruptor 'seguir alterações' de um monitor
// Keep props, emits and slots in sync with this component's entry in components.json.
// No global state and no network: data comes in through props, actions go out through emits.
import { defineComponent } from "vue"

export default defineComponent({
    name: "FollowSwitch",
    props: {
        on: { type: Boolean, default: false },
        disabled: { type: Boolean, default: false },
        label: { type: String, default: "Seguir" },
    },
    emits: ["toggle"],
    template: `
        <button type="button" role="switch" :aria-checked="on" :disabled="disabled"
                class="inline-flex min-h-9 items-center gap-2 rounded-md border px-2.5 py-1.5 text-xs font-medium transition-colors disabled:cursor-not-allowed disabled:opacity-50"
                :class="on ? 'border-primary bg-accent text-foreground' : 'border-border bg-white text-muted-foreground hover:border-primary/60'"
                @click.stop="$emit('toggle')">
            <span class="relative inline-flex h-4 w-7 shrink-0 rounded-full transition-colors" :class="on ? 'bg-primary' : 'bg-slate-300'">
                <span class="absolute top-0.5 size-3 rounded-full bg-white transition-all" :class="on ? 'left-3.5' : 'left-0.5'"></span>
            </span>
            <mock-icon name="link" :size="14"></mock-icon>
            {{ label }}
        </button>
    `,
})
