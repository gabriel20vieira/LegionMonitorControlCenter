// DisplayMap: Disposição dos ecrãs à escala, clicável, com marca de seleção e de seguir
// Keep props, emits and slots in sync with this component's entry in components.json.
// No global state and no network: data comes in through props, actions go out through emits.
import { defineComponent } from "vue"

export default defineComponent({
    name: "DisplayMap",
    props: {
        // [{ key, number, name, x, y, w, h }] in desktop pixels
        monitors: { type: Array, required: true },
        selected: { type: String, default: "" },
        following: { type: Array, default: () => [] },
        height: { type: Number, default: 150 },
    },
    emits: ["select"],
    computed: {
        box() {
            const xs = this.monitors.flatMap((m) => [m.x, m.x + m.w])
            const ys = this.monitors.flatMap((m) => [m.y, m.y + m.h])
            return { x: Math.min(...xs), y: Math.min(...ys), w: Math.max(...xs) - Math.min(...xs), h: Math.max(...ys) - Math.min(...ys) }
        },
    },
    methods: {
        style(m) {
            const b = this.box
            return {
                left: ((m.x - b.x) / b.w) * 100 + "%",
                top: ((m.y - b.y) / b.h) * 100 + "%",
                width: (m.w / b.w) * 100 + "%",
                height: (m.h / b.h) * 100 + "%",
            }
        },
    },
    template: `
        <div class="relative mx-auto w-full" :style="{ aspectRatio: box.w + ' / ' + box.h, maxHeight: height + 'px', maxWidth: (height * box.w / box.h) + 'px' }">
            <button v-for="m in monitors" :key="m.key" type="button"
                    class="absolute flex flex-col items-center justify-center gap-0.5 rounded-md border-2 p-1 transition-colors"
                    :class="m.key === selected ? 'border-primary bg-primary text-primary-foreground shadow-md'
                          : following.includes(m.key) ? 'border-primary bg-accent text-foreground'
                          : 'border-border bg-muted text-foreground hover:border-primary/60'"
                    :style="[style(m), { margin: '0 3px', width: 'calc(' + style(m).width + ' - 6px)' }]"
                    :title="m.name"
                    @click="$emit('select', m.key)">
                <span class="text-xl font-semibold leading-none @3xl:text-2xl">{{ m.number }}</span>
                <span class="block max-w-full truncate px-1 text-[11px] opacity-80">{{ m.name }}</span>
                <span v-if="following.includes(m.key) && m.key !== selected" class="absolute right-1 top-1 inline-flex items-center gap-0.5 rounded bg-primary px-1 text-[10px] font-medium text-primary-foreground">
                    <mock-icon name="link" :size="10"></mock-icon><span class="hidden @3xl:inline">segue</span>
                </span>
            </button>
        </div>
    `,
})
