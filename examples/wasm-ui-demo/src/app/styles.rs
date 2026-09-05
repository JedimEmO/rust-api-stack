use dominator::class;
use once_cell::sync::Lazy;

// Define styles using dominator's class! macro
pub(super) static STYLES: Lazy<String> = Lazy::new(|| {
    class! {
        .raw("
            * {
                box-sizing: border-box;
            }
            
            body {
                margin: 0;
                font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Oxygen, Ubuntu, Cantarell, sans-serif;
                background-color: #0a0a0a;
                color: #e5e5e5;
                line-height: 1.5;
            }
            
            /* Custom scrollbar for dark mode */
            ::-webkit-scrollbar {
                width: 8px;
                height: 8px;
            }
            
            ::-webkit-scrollbar-track {
                background: #1a1a1a;
            }
            
            ::-webkit-scrollbar-thumb {
                background: #404040;
                border-radius: 4px;
            }
            
            ::-webkit-scrollbar-thumb:hover {
                background: #555;
            }
            
            /* Glass morphism effect */
            .glass {
                background: rgba(255, 255, 255, 0.05);
                backdrop-filter: blur(10px);
                border: 1px solid rgba(255, 255, 255, 0.1);
            }
            
            /* Smooth transitions */
            * {
                transition: all 0.2s ease;
            }
            
            /* Animations */
            @keyframes fadeIn {
                from { opacity: 0; transform: translateY(10px); }
                to { opacity: 1; transform: translateY(0); }
            }
            
            @keyframes slideIn {
                from { transform: translateX(-100%); }
                to { transform: translateX(0); }
            }
            
            @keyframes pulse {
                0%, 100% { opacity: 1; }
                50% { opacity: 0.8; }
            }
            
            .animate-fade-in {
                animation: fadeIn 0.5s ease-out;
            }
            
            .animate-slide-in {
                animation: slideIn 0.3s ease-out;
            }
            
            .animate-pulse {
                animation: pulse 2s infinite;
            }
        ")
    }
});
