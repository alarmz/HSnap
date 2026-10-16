import ghidra.app.script.GhidraScript;
import ghidra.program.model.listing.*;
import ghidra.program.model.symbol.*;
import ghidra.program.model.data.*;
import java.io.*;
import java.util.*;

public class ExportInfo extends GhidraScript {
    @Override
    public void run() throws Exception {
        String dir = getScriptArgs().length > 0 ? getScriptArgs()[0] : ".";
        // Imports grouped by library
        TreeMap<String, TreeSet<String>> imports = new TreeMap<>();
        SymbolIterator ext = currentProgram.getSymbolTable().getExternalSymbols();
        while (ext.hasNext()) {
            Symbol s = ext.next();
            ExternalLocation loc = currentProgram.getExternalManager().getExternalLocation(s);
            String lib = loc != null ? loc.getLibraryName() : "?";
            imports.computeIfAbsent(lib, k -> new TreeSet<>()).add(s.getName());
        }
        try (PrintWriter w = new PrintWriter(new File(dir, "imports.txt"))) {
            for (var e : imports.entrySet()) {
                w.println("[" + e.getKey() + "]");
                for (String n : e.getValue()) w.println("  " + n);
            }
        }
        // Defined strings
        try (PrintWriter w = new PrintWriter(new File(dir, "strings.txt"), "UTF-8")) {
            DataIterator di = currentProgram.getListing().getDefinedData(true);
            while (di.hasNext()) {
                Data d = di.next();
                if (d.getValue() instanceof String) {
                    w.println(d.getAddress() + "\t" + d.getValue().toString().replace("\n", "\\n").replace("\r", "\\r"));
                }
            }
        }
        // Functions with callers to key APIs
        try (PrintWriter w = new PrintWriter(new File(dir, "functions.txt"))) {
            FunctionIterator fi = currentProgram.getListing().getFunctions(true);
            int n = 0;
            while (fi.hasNext()) {
                Function f = fi.next();
                n++;
                w.println(f.getEntryPoint() + "\t" + f.getName() + "\t" + f.getBody().getNumAddresses());
            }
            println("functions: " + n);
        }
        // Callers of interesting APIs
        String[] interesting = {"BitBlt","StretchBlt","GetDC","GetWindowDC","CreateCompatibleBitmap","RegisterHotKey",
            "SetWindowsHookExA","SetWindowsHookExW","GetForegroundWindow","WindowFromPoint","GetWindowRect","PrintWindow",
            "OpenClipboard","SetClipboardData","GdiplusStartup","CreateDIBSection","GetDIBits","SetCursorPos","DwmGetWindowAttribute",
            "EnumDisplayMonitors","GetSystemMetrics","SHGetFolderPathA","RegSetValueExA"};
        try (PrintWriter w = new PrintWriter(new File(dir, "api_callers.txt"))) {
            for (String api : interesting) {
                SymbolIterator it = currentProgram.getSymbolTable().getSymbols(api);
                while (it.hasNext()) {
                    Symbol s = it.next();
                    Set<String> callers = new TreeSet<>();
                    for (Reference r : getReferencesTo(s.getAddress())) {
                        Function f = getFunctionContaining(r.getFromAddress());
                        callers.add(f != null ? f.getName() + "@" + f.getEntryPoint() : r.getFromAddress().toString());
                    }
                    w.println(api + " (" + callers.size() + ")");
                    for (String c : callers) w.println("  " + c);
                }
            }
        }
    }
}
