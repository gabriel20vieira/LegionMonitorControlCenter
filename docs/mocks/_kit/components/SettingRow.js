// SettingRow: Linha de definição: deslizador 0-100 ou lista de valores
// Keep props, emits and slots in sync with this component's entry in components.json.
// No global state and no network: data comes in through props, actions go out through emits.
import { defineComponent } from "vue"

export default defineComponent({
    name: "SettingRow",
    props: {
        label: { type: String, required: true },
        icon: { type: String, default: "" },
        // number for a slider, string for a choice
        value: { type: [Number, String], required: true },
        choices: { type: Array, default: null },
        max: { type: Number, default: 100 },
        hint: { type: String, default: "" },
    },
    emits: ["change"],
    template: `
        <div class="flex flex-col gap-2 py-3 @xl:flex-row @xl:items-center @xl:gap-4">
            <div class="flex min-w-0 items-center gap-2 @xl:w-48 @xl:shrink-0">
                <mock-icon v-if="icon" :name="icon" :size="16" class="text-muted-foreground"></mock-icon>
                <span class="text-sm font-medium">{{ label }}</span>
            </div>
            <div v-if="choices" class="flex-1">
                <select class="w-full rounded-md border border-border bg-white px-3 py-2 text-sm @xl:max-w-xs"
                        :value="value" @change="$emit('change', $event.target.value)">
                    <option v-for="c in choices" :key="c" :value="c">{{ c }}</option>
                </select>
            </div>
            <div v-else class="flex flex-1 items-center gap-3">
                <input type="range" min="0" :max="max" :value="value" class="h-2 flex-1 accent-primary"
                       @input="$emit('change', Number($event.target.value))">
                <span class="w-10 text-right text-sm tabular-nums text-muted-foreground">{{ value }}</span>
            </div>
            <p v-if="hint" class="text-xs text-muted-foreground @xl:w-40">{{ hint }}</p>
        </div>
    `,
})
