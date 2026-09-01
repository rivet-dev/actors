using MonoFN.Cecil;
using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Threading;
using Unity.CompilationPipeline.Common.ILPostProcessing;

namespace FishNet.CodeGenerating
{
    internal class FNPostProcessorAssemblyResolver : IAssemblyResolver
    {
        private readonly string[] m_AssemblyReferences;
        private readonly Dictionary<string, AssemblyDefinition> m_AssemblyCache = new();
        private readonly ICompiledAssembly m_CompiledAssembly;
        private AssemblyDefinition m_SelfAssembly;

        public FNPostProcessorAssemblyResolver(ICompiledAssembly compiledAssembly)
        {
            m_CompiledAssembly = compiledAssembly;
            m_AssemblyReferences = compiledAssembly.References;
        }

        public void Dispose() { }
        public AssemblyDefinition Resolve(AssemblyNameReference name) => Resolve(name, new(ReadingMode.Deferred));

        public AssemblyDefinition Resolve(AssemblyNameReference name, ReaderParameters parameters)
        {
            lock (m_AssemblyCache)
            {
                if (name.Name == m_CompiledAssembly.Name)
                {
                    return m_SelfAssembly;
                }

                string fileName = FindFile(name);
                if (fileName == null)
                {
                    return null;
                }

                DateTime lastWriteTime = File.GetLastWriteTime(fileName);
                string cacheKey = $"{fileName}{lastWriteTime}";
                if (m_AssemblyCache.TryGetValue(cacheKey, out AssemblyDefinition result))
                {
                    return result;
                }

                parameters.AssemblyResolver = this;

                MemoryStream ms = MemoryStreamFor(fileName);
                string pdb = $"{fileName}.pdb";
                if (File.Exists(pdb))
                {
                    parameters.SymbolStream = MemoryStreamFor(pdb);
                }

                AssemblyDefinition assemblyDefinition = AssemblyDefinition.ReadAssembly(ms, parameters);
                m_AssemblyCache.Add(cacheKey, assemblyDefinition);

                return assemblyDefinition;
            }
        }

        private string FindFile(AssemblyNameReference name)
        {
            string fileName = m_AssemblyReferences.FirstOrDefault(r => Path.GetFileName(r) == $"{name.Name}.dll");
            if (fileName != null)
            {
                return fileName;
            }

            // perhaps the type comes from an exe instead
            fileName = m_AssemblyReferences.FirstOrDefault(r => Path.GetFileName(r) == $"{name.Name}.exe");
            if (fileName != null)
            {
                return fileName;
            }

            // Unfortunately the current ICompiledAssembly API only provides direct references.
            // It is very much possible that a postprocessor ends up investigating a type in a directly
            //referenced assembly, that contains a field that is not in a directly referenced assembly.
            //if we don't do anything special for that situation, it will fail to resolve.  We should fix this
            //in the ILPostProcessing API. As a workaround, we rely on the fact here that the indirect references
            //are always located next to direct references, so we search in all directories of direct references we
            //got passed, and if we find the file in there, we resolve to it.
            return m_AssemblyReferences.Select(Path.GetDirectoryName).Distinct().Select(parentDir => Path.Combine(parentDir, $"{name.Name}.dll")).FirstOrDefault(File.Exists);
        }

        private static MemoryStream MemoryStreamFor(string fileName)
        {
            return Retry(10, TimeSpan.FromSeconds(1), () =>
            {
                byte[] byteArray;
                using (FileStream fs = new(fileName, FileMode.Open, FileAccess.Read, FileShare.ReadWrite))
                {
                    byteArray = new byte[fs.Length];
                    int readLength = fs.Read(byteArray, 0, (int)fs.Length);
                    if (readLength != fs.Length)
                    {
                        throw new InvalidOperationException("File read length is not full length of file.");
                    }
                }

                return new(byteArray);
            });
        }

        private static MemoryStream Retry(int retryCount, TimeSpan waitTime, Func<MemoryStream> func)
        {
            try
            {
                return func();
            }
            catch (IOException)
            {
                if (retryCount == 0)
                {
                    throw;
                }

                Console.WriteLine($"Caught IO Exception, trying {retryCount} more times");
                Thread.Sleep(waitTime);

                return Retry(retryCount - 1, waitTime, func);
            }
        }

        public void AddAssemblyDefinitionBeingOperatedOn(AssemblyDefinition assemblyDefinition)
        {
            m_SelfAssembly = assemblyDefinition;
        }
    }
}