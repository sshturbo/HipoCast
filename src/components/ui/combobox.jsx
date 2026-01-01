import * as React from "react"
import { Check, ChevronsUpDown } from "lucide-react"
import { cn } from "@/lib/utils"
import { Button } from "@/components/ui/button"

export function Combobox({ value, onChange, options, placeholder = "Selecione uma opção...", disabled = false, className }) {
    const [open, setOpen] = React.useState(false)
    const [search, setSearch] = React.useState("")
    const containerRef = React.useRef(null)

    const selectedOption = options.find(opt => opt.value === value)

    const filteredOptions = options.filter(option =>
        option.label.toLowerCase().includes(search.toLowerCase())
    )

    React.useEffect(() => {
        const handleClickOutside = (event) => {
            if (containerRef.current && !containerRef.current.contains(event.target)) {
                setOpen(false)
            }
        }

        if (open) {
            document.addEventListener('mousedown', handleClickOutside)
        }

        return () => {
            document.removeEventListener('mousedown', handleClickOutside)
        }
    }, [open])

    return (
        <div ref={containerRef} className={cn("relative w-full", className)}>
            <Button
                type="button"
                onClick={() => !disabled && setOpen(!open)}
                disabled={disabled}
                className={cn(
                    "w-full justify-between bg-slate-900/20 backdrop-blur-md border border-slate-700/50 hover:border-slate-600 text-white hover:bg-slate-900/30 h-11 rounded-xl px-4",
                    disabled && "opacity-50 cursor-not-allowed"
                )}
            >
                <span className={cn("truncate", !selectedOption && "text-slate-500")}>
                    {selectedOption ? selectedOption.label : placeholder}
                </span>
                <ChevronsUpDown className="ml-2 h-4 w-4 shrink-0 opacity-50" />
            </Button>

            {open && (
                <div className="absolute z-50 w-full mt-2 bg-slate-900 border border-slate-700 rounded-xl shadow-2xl overflow-hidden">
                    <div className="p-2 border-b border-slate-800">
                        <input
                            type="text"
                            placeholder="Buscar..."
                            value={search}
                            onChange={(e) => setSearch(e.target.value)}
                            className="w-full px-3 py-2 bg-slate-950/50 border border-slate-700 rounded-lg text-sm text-white placeholder:text-slate-500 focus:outline-none focus:ring-2 focus:ring-violet-500/30"
                            autoFocus
                        />
                    </div>

                    <div className="max-h-[300px] overflow-y-auto custom-scrollbar">
                        {filteredOptions.length === 0 ? (
                            <div className="px-4 py-6 text-center text-sm text-slate-500">
                                Nenhum resultado encontrado
                            </div>
                        ) : (
                            <div className="p-1">
                                {filteredOptions.map((option) => (
                                    <button
                                        key={option.value}
                                        type="button"
                                        onClick={() => {
                                            onChange(option.value)
                                            setOpen(false)
                                            setSearch("")
                                        }}
                                        className={cn(
                                            "w-full flex items-center justify-between px-3 py-2.5 text-sm rounded-lg transition-colors",
                                            value === option.value
                                                ? "bg-violet-600 text-white"
                                                : "text-slate-300 hover:bg-slate-800"
                                        )}
                                    >
                                        <span className="truncate">{option.label}</span>
                                        {value === option.value && (
                                            <Check className="ml-2 h-4 w-4 shrink-0" />
                                        )}
                                    </button>
                                ))}
                            </div>
                        )}
                    </div>
                </div>
            )}
        </div>
    )
}
