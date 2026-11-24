// MonitorBadge: Número do Windows de um monitor num círculo
// Keep props, emits and slots in sync with this component's entry in components.json.
// No global state and no network: data comes in through props, actions go out through emits.
import { defineComponent } from "vue"

export default defineComponent({
    name: "MonitorBadge",
    props: {
        number: { type: Number, required: true },
        active: { type: Boolean, default: false },
        size: { type: String, default: "md" }, // sm | md | lg
    },
    computed: {
        sizeClass() {
            return { sm: "size-7 text-sm", md: "size-9 text-base", lg: "size-14 text-2xl" }[this.size] ?? "size-9 text-base"
        },
    },
    template: `
        <span class="inline-flex shrink-0 items-center justify-center rounded-full font-semibold"
              :class="[sizeClass, active ? 'bg-primary text-primary-foreground' : 'bg-muted text-foreground border border-border']">
            {{ number }}
        </span>
    `,
})
