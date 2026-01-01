import { Home, Settings, Info, Video, HelpCircle } from 'lucide-react';
import { cn } from '@/lib/utils';

export default function Sidebar({ activeTab, onTabChange }) {
    const menuItems = [
        { id: 'streaming', label: 'Streaming', icon: Home },
        { id: 'settings', label: 'Configurações', icon: Settings },
        { id: 'help', label: 'Ajuda', icon: HelpCircle },
        { id: 'about', label: 'Sobre', icon: Info },
    ];

    return (
        <aside className="w-64 border-r border-slate-800 bg-slate-900/50 backdrop-blur-xl flex flex-col h-screen sticky top-0">
            <div className="p-6 flex items-center gap-3">
                <div className="w-10 h-10 rounded-xl bg-gradient-to-br from-violet-500 to-purple-600 flex items-center justify-center shadow-lg shadow-violet-500/25">
                    <Video className="w-6 h-6 text-white" />
                </div>
                <span className="font-bold text-xl tracking-tight text-white">HipoCast</span>
            </div>

            <nav className="flex-1 px-4 py-4 space-y-2">
                {menuItems.map((item) => (
                    <button
                        key={item.id}
                        onClick={() => onTabChange(item.id)}
                        className={cn(
                            "w-full flex items-center gap-3 px-4 py-3 rounded-lg transition-all duration-200 group",
                            activeTab === item.id
                                ? "bg-violet-600 text-white shadow-lg shadow-violet-600/20"
                                : "text-slate-400 hover:bg-slate-800 hover:text-white"
                        )}
                    >
                        <item.icon className={cn(
                            "w-5 h-5",
                            activeTab === item.id ? "text-white" : "text-slate-500 group-hover:text-violet-400"
                        )} />
                        <span className="font-medium">{item.label}</span>
                    </button>
                ))}
            </nav>
        </aside>
    );
}
