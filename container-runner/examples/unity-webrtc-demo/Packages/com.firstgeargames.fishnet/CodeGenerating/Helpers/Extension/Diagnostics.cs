using MonoFN.Cecil;
using MonoFN.Cecil.Cil;
using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using Unity.CompilationPipeline.Common.Diagnostics;

namespace FishNet.CodeGenerating.Helping
{
    internal static class Diagnostics
    {
        internal static void AddError(this List<DiagnosticMessage> diagnostics, string message)
        {
            diagnostics.AddMessage(DiagnosticType.Error, (SequencePoint)null, message);
        }

        internal static void AddWarning(this List<DiagnosticMessage> diagnostics, string message)
        {
            diagnostics.AddMessage(DiagnosticType.Warning, (SequencePoint)null, message);
        }

        internal static void AddError(this List<DiagnosticMessage> diagnostics, MethodDefinition methodDef, string message)
        {
            diagnostics.AddMessage(DiagnosticType.Error, methodDef.DebugInformation.SequencePoints.FirstOrDefault(), message);
        }

        internal static void AddMessage(this List<DiagnosticMessage> diagnostics, DiagnosticType diagnosticType, SequencePoint sequencePoint, string message)
        {
            diagnostics.Add(new()
            {
                DiagnosticType = diagnosticType,
                File = sequencePoint?.Document.Url.Replace($"{Environment.CurrentDirectory}{Path.DirectorySeparatorChar}", ""),
                Line = sequencePoint?.StartLine ?? 0,
                Column = sequencePoint?.StartColumn ?? 0,
                MessageData = $" - {message}"
            });
        }
    }
}