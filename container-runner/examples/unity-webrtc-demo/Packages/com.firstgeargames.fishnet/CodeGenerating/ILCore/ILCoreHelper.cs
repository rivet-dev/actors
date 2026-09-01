using MonoFN.Cecil;
using MonoFN.Cecil.Cil;
using System.IO;
using Unity.CompilationPipeline.Common.ILPostProcessing;

namespace FishNet.CodeGenerating.ILCore
{
    internal static class ILCoreHelper
    {
        /// <summary>
        /// Returns AssembleDefinition for compiledAssembly.
        /// </summary>
        /// <param name = "compiledAssembly"></param>
        /// <returns></returns>
        internal static AssemblyDefinition GetAssemblyDefinition(ICompiledAssembly compiledAssembly)
        {
            FNPostProcessorAssemblyResolver assemblyResolver = new(compiledAssembly);
            ReaderParameters readerParameters = new()
            {
                SymbolStream = new MemoryStream(compiledAssembly.InMemoryAssembly.PdbData),
                SymbolReaderProvider = new PortablePdbReaderProvider(),
                AssemblyResolver = assemblyResolver,
                ReflectionImporterProvider = new FNPostProcessorReflectionImporterProvider(),
                ReadingMode = ReadingMode.Immediate
            };

            AssemblyDefinition assemblyDefinition = AssemblyDefinition.ReadAssembly(new MemoryStream(compiledAssembly.InMemoryAssembly.PeData), readerParameters);
            // Allows us to resolve inside FishNet assembly, such as for components.
            assemblyResolver.AddAssemblyDefinitionBeingOperatedOn(assemblyDefinition);

            return assemblyDefinition;
        }
    }
}