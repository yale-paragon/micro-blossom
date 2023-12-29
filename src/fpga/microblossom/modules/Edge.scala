package microblossom.modules

import spinal.core._
import spinal.lib._
import microblossom._
import microblossom.types._
import microblossom.stage._
import microblossom.combinatorial._
import microblossom.util.Vivado
import org.scalatest.funsuite.AnyFunSuite

object Edge {
  def getStages(
      config: DualConfig
  ): Stages[
    StageOffloadEdge,
    StageOffloadEdge2,
    StageOffloadEdge3,
    StageOffloadEdge4,
    StageExecuteEdge,
    StageExecuteEdge2,
    StageExecuteEdge3,
    StageUpdateEdge,
    StageUpdateEdge2,
    StageUpdateEdge3
  ] = {
    Stages(
      offload = () => StageOffloadEdge(config),
      offload2 = () => StageOffloadEdge2(config),
      offload3 = () => StageOffloadEdge3(config),
      offload4 = () => StageOffloadEdge4(config),
      execute = () => StageExecuteEdge(config),
      execute2 = () => StageExecuteEdge2(config),
      execute3 = () => StageExecuteEdge3(config),
      update = () => StageUpdateEdge(config),
      update2 = () => StageUpdateEdge2(config),
      update3 = () => StageUpdateEdge3(config)
    )
  }
}

case class Edge(config: DualConfig, edgeIndex: Int, injectRegisters: Seq[String] = List()) extends Component {
  val (leftVertex, rightVertex) = config.incidentVerticesOf(edgeIndex)
  val leftGrownBits = config.grownBitsOf(leftVertex)
  val rightGrownBits = config.grownBitsOf(rightVertex)

  val io = new Bundle {
    val message = in(BroadcastMessage(config))
    // interaction I/O
    val stageOutputs = out(Edge.getStages(config).getStageOutput)
    val leftVertexInput = in(Vertex.getStages(config, leftVertex).getStageOutput)
    val rightVertexInput = in(Vertex.getStages(config, rightVertex).getStageOutput)
    // final outputs
    val maxLength = out(ConvergecastMaxLength(config.weightBits))
    val conflict = out(ConvergecastConflict(config.vertexBits))
  }

  val stages = Edge.getStages(config)
  stages.connectStageOutput(io.stageOutputs)

  // fetch
  var ram: Mem[EdgeState] = null
  var register = Reg(EdgeState(config.weightBits))
  var fetchState = EdgeState(config.weightBits)
  var message = BroadcastMessage(config)
  if (config.contextBits > 0) {
    // fetch stage, delay the instruction
    ram = Mem(EdgeState(config.weightBits), config.contextDepth)
    fetchState := ram.readSync(
      address = io.message.contextId,
      enable = io.message.valid
    )
    message := RegNext(io.message)
  } else {
    fetchState := register
    message := io.message
  }

  // mock
  stages.offloadSet.state := fetchState
  stages.offloadSet.valid := message.valid
  if (config.contextBits > 0) {
    stages.offloadSet.contextId := message.contextId
  }

  stages.offloadSet2.connect(stages.offloadGet)
  val offload2Area = new Area {
    val edgeIsTight = EdgeIsTight(
      leftGrownBits = leftGrownBits,
      rightGrownBits = rightGrownBits,
      weightBits = config.weightBits
    )
    edgeIsTight.io.leftGrown := io.leftVertexInput.offloadGet.state.grown
    edgeIsTight.io.rightGrown := io.rightVertexInput.offloadGet.state.grown
    edgeIsTight.io.weight := stages.offloadGet.state.weight
    stages.offloadSet2.isTight := edgeIsTight.io.isTight
  }

  stages.offloadSet3.connect(stages.offloadGet2)

  stages.offloadSet4.connect(stages.offloadGet3)

  stages.executeSet.connect(stages.offloadGet4)

  stages.executeSet2.connect(stages.executeGet)

  stages.executeSet3.connect(stages.executeGet2)
  val execute3Area = new Area {
    val edgeIsTight = EdgeIsTight(
      leftGrownBits = leftGrownBits,
      rightGrownBits = rightGrownBits,
      weightBits = config.weightBits
    )
    edgeIsTight.io.leftGrown := io.leftVertexInput.executeGet2.state.grown
    edgeIsTight.io.rightGrown := io.rightVertexInput.executeGet2.state.grown
    edgeIsTight.io.weight := stages.executeGet2.state.weight
    stages.executeSet3.isTight := edgeIsTight.io.isTight
  }

  stages.updateSet.connect(stages.executeGet3)

  stages.updateSet2.connect(stages.updateGet)

  stages.updateSet3.connect(stages.updateGet2)
  val update3Area = new Area {
    val edgeRemaining = EdgeRemaining(
      leftGrownBits = leftGrownBits,
      rightGrownBits = rightGrownBits,
      weightBits = config.weightBits
    )
    edgeRemaining.io.leftGrown := io.leftVertexInput.updateGet2.state.grown
    edgeRemaining.io.rightGrown := io.rightVertexInput.updateGet2.state.grown
    edgeRemaining.io.weight := stages.updateGet2.state.weight
    stages.updateSet3.remaining := edgeRemaining.io.remaining
  }

  val edgeResponse = EdgeResponse(config.vertexBits, config.weightBits)
  edgeResponse.io.leftShadow := io.leftVertexInput.updateGet3.shadow
  edgeResponse.io.rightShadow := io.rightVertexInput.updateGet3.shadow
  edgeResponse.io.leftVertex := leftVertex
  edgeResponse.io.rightVertex := rightVertex
  edgeResponse.io.remaining := stages.updateGet3.remaining
  io.maxLength := edgeResponse.io.maxLength
  io.conflict := edgeResponse.io.conflict

  // TODO: write back
  if (config.contextBits > 0) {
    ram.write(
      address = stages.updateGet3.contextId,
      data = stages.updateGet3.state,
      enable = stages.updateGet3.valid
    )
  } else {
    when(stages.updateGet3.valid) {
      register := stages.updateGet3.state
    }
  }

  // inject registers
  for (stageName <- injectRegisters) {
    stages.injectRegisterAt(stageName)
  }
  stages.finish()

}

// sbt 'testOnly microblossom.modules.EdgeTest'
class EdgeTest extends AnyFunSuite {

  test("construct a Edge") {
    val config = DualConfig(filename = "./resources/graphs/example_code_capacity_d3.json")
    // config.contextDepth = 1024 // fit in a single Block RAM of 36 kbits in 36-bit mode
    config.contextDepth = 1 // no context switch
    config.sanityCheck()
    Config.spinal().generateVerilog(Edge(config, 0))
  }

}

// sbt 'testOnly microblossom.modules.EdgeResourceEstimation'
class EdgeResourceEstimation extends AnyFunSuite {

  test("logic delay") {
    val configurations = List(
      (
        DualConfig(filename = "./resources/graphs/example_code_capacity_d5.json"),
        1,
        "code capacity 2 neighbors"
      ), //
      (
        DualConfig(filename = "./resources/graphs/example_code_capacity_rotated_d5.json"),
        12,
        "code capacity 4 neighbors"
      ), //
      (
        DualConfig(filename = "./resources/graphs/example_phenomenological_rotated_d5.json"),
        141,
        "phenomenological 6 neighbors"
      ), //
      (
        DualConfig(filename = "./resources/graphs/example_circuit_level_d5.json"),
        365,
        "circuit-level 12 neighbors"
      ) //
    )
    for ((config, edgeIndex, name) <- configurations) {
      val timingReport = Vivado.reportTiming(Edge(config, edgeIndex))
      println(s"$name: ${timingReport.getPathDelaysExcludingIOWorst}ns")
    }
  }

}
